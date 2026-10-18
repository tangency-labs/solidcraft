//! Runner for unit `curve.curve_p3`: `SmCurve` part 3 (the normal of a curve loop,
//! closing chain gaps, arc length stepping, the local point / curve / intersection
//! solvers, `IsLinear` co-ported from unit `curve.curve_p5`), the local solve of the
//! curve property global solver and the evaluator objects `SmFindPCExtremaEFO` and
//! `SmBestLineFitENFO`.
//!
//! C++: `solidcore_ffi::units::curve_curve_p3` (upstream `source/SMLib/src/SmCurve.cpp`,
//! called through `SmCurve*` on B-spline, Hermite and line curves; the local classes are
//! verbatim copies in the shim). Rust: `solidcore::curve::curve::p3`.
//!
//! A case is one `op` with its curves (`{"bspline": {dim, oob, cpts, p, u}}` with
//! control points as stored `[x, y, z, w]`, `{"hermite": {dim, p1, d1, p2, d2}}`,
//! `{"circle": ..}` (a quadratic B-spline arc) or `{"line": {p, v, domain, scale,
//! inside_out}}`) and parameters. Both sides produce the flat record documented in the
//! ffi module; the runner reports its first entry as the `status` int (`0` =
//! `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a Rust
//! error upstream cannot produce) and the rest as the `out` values.
//!
//! Deferred cases (unit `curve.curve_p5`): arc lengths on curves other than lines
//! (`SmCurve::Length`), the ray point solves and property solves that reach
//! `SmCurve::LocalPropertyAnalysis`, and every curve/curve solve and intersection
//! (`SmFindCCNormalENFO::Evaluate`, `SmCCIntersectENFO::Evaluate`). `CrvsFixGaps`
//! cases that would delete a duplicate curve are not generated (a use after free
//! upstream, see `plan/findings.md`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::{Curve, CurveKind, CurvePropertyGs, CurvePropertyType};
use solidcore::curve::curve::p3::{
    compute_crvs_normal, crvs_fix_gaps, local_curve_intersect, BestLineFitEnfo,
    PointSolveOptions,
};
use solidcore::curve::curve::p4::{fix_chain_curves_list, FindPcExtremaEfo};
use solidcore::curve::curve_cache::{CurveCache, CurveCacheOptions};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::global_solver::{Branch, GlobalSolve, SolverOperation};
use solidcore::geom::local_solve1d::{EvalFunction, FunctionValue};
use solidcore::geom::local_solve_nd::EvalNFunctions;
use solidcore::geom::matrix::Matrix;
use solidcore::geom::solution_array::{ObjectKind, Solution, SolutionObject, SolutionType};
use solidcore::geom::tree::{CurveSpanRef, NodeData, NodeId, Tree};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::SolidError;
use solidcore_ffi::units::curve_curve_p3::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `curve.curve_p3` runner.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CurveIn {
    Bspline(BSplineIn),
    Hermite(HermiteIn),
    Circle(CircleIn),
    Line(LineIn),
}

/// `SmCurveCache` constructor arguments.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheIn {
    ivl: [f64; 2],
    #[serde(default)]
    full: bool,
    #[serde(default)]
    chord: f64,
    #[serde(default)]
    angle: f64,
    #[serde(default)]
    offaxis: f64,
    #[serde(default)]
    minseg: u64,
    #[serde(default)]
    aux: bool,
    #[serde(default)]
    max3d: f64,
    #[serde(default = "default_ratio")]
    ratio: f64,
}

fn default_ratio() -> f64 {
    1e-4
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Normal {
        curves: Vec<CurveIn>,
    },
    FixGaps {
        curves: Vec<CurveIn>,
        tol: f64,
    },
    ArcLength {
        curve: CurveIn,
        start: f64,
        len: f64,
    },
    PcExtrema {
        curve: CurveIn,
        p: [f64; 3],
        dom: [f64; 2],
        sqtol: f64,
        sop: i32,
        ts: Vec<f64>,
    },
    LineFit {
        c1: CurveIn,
        c2: CurveIn,
        x: [f64; 2],
        jac: bool,
    },
    PointSolve {
        curve: CurveIn,
        ivl: [f64; 2],
        sop: i32,
        p: [f64; 3],
        dist_tol: Option<f64>,
        target: Option<f64>,
        vecs: Vec<[f64; 3]>,
        guess: f64,
    },
    CurveSolve {
        c1: CurveIn,
        ivl1: [f64; 2],
        c2: CurveIn,
        ivl2: [f64; 2],
        sop: i32,
        tol: f64,
        target: Option<f64>,
        vecs: Vec<[f64; 3]>,
        g: [f64; 2],
    },
    Intersect {
        c1: CurveIn,
        ivl1: [f64; 2],
        c2: CurveIn,
        ivl2: [f64; 2],
        tol: f64,
        g: [f64; 2],
    },
    PropertyLocal {
        curve: CurveIn,
        cache: CacheIn,
        ivl: [f64; 2],
        prop: i32,
        val: f64,
        vecs: Vec<[f64; 3]>,
        sop: i32,
        tol: f64,
        best: f64,
        leaf: u64,
    },
    IsLinear {
        curve: CurveIn,
        tol: f64,
        want: bool,
    },
}

// ---- C++ input --------------------------------------------------------------------

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

fn op_of(op: &OpIn) -> Op {
    match op {
        OpIn::Normal { .. } => Op::Normal,
        OpIn::FixGaps { .. } => Op::FixGaps,
        OpIn::ArcLength { .. } => Op::ArcLength,
        OpIn::PcExtrema { .. } => Op::PcExtrema,
        OpIn::LineFit { .. } => Op::LineFit,
        OpIn::PointSolve { .. } => Op::PointSolve,
        OpIn::CurveSolve { .. } => Op::CurveSolve,
        OpIn::Intersect { .. } => Op::Intersect,
        OpIn::PropertyLocal { .. } => Op::PropertyLocal,
        OpIn::IsLinear { .. } => Op::IsLinear,
    }
}

fn push_vecs(v: &mut Vec<f64>, vecs: &[[f64; 3]]) {
    v.push(vecs.len() as f64);
    for x in vecs {
        v.extend_from_slice(x);
    }
}

fn push_opt(v: &mut Vec<f64>, x: Option<f64>) {
    v.extend([b(x.is_some()), x.unwrap_or(0.0)]);
}

fn cpp_input(op: &OpIn) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut v = Vec::new();
    match op {
        OpIn::Normal { curves } | OpIn::FixGaps { curves, .. } => {
            v.push(curves.len() as f64);
            for c in curves {
                curve_block(&mut v, c)?;
            }
            if let OpIn::FixGaps { tol, .. } = op {
                v.push(*tol);
            }
        }
        OpIn::ArcLength { curve, start, len } => {
            curve_block(&mut v, curve)?;
            v.extend([*start, *len]);
        }
        OpIn::PcExtrema {
            curve,
            p,
            dom,
            sqtol,
            sop,
            ts,
        } => {
            curve_block(&mut v, curve)?;
            v.extend_from_slice(p);
            v.extend([dom[0], dom[1], *sqtol, f64::from(*sop), ts.len() as f64]);
            v.extend_from_slice(ts);
        }
        OpIn::LineFit { c1, c2, x, jac } => {
            curve_block(&mut v, c1)?;
            curve_block(&mut v, c2)?;
            v.extend([x[0], x[1], b(*jac)]);
        }
        OpIn::PointSolve {
            curve,
            ivl,
            sop,
            p,
            dist_tol,
            target,
            vecs,
            guess,
        } => {
            curve_block(&mut v, curve)?;
            v.extend([ivl[0], ivl[1], f64::from(*sop)]);
            v.extend_from_slice(p);
            push_opt(&mut v, *dist_tol);
            push_opt(&mut v, *target);
            push_vecs(&mut v, vecs);
            v.push(*guess);
        }
        OpIn::CurveSolve {
            c1,
            ivl1,
            c2,
            ivl2,
            sop,
            tol,
            target,
            vecs,
            g,
        } => {
            curve_block(&mut v, c1)?;
            v.extend_from_slice(ivl1);
            curve_block(&mut v, c2)?;
            v.extend_from_slice(ivl2);
            v.extend([f64::from(*sop), *tol]);
            push_opt(&mut v, *target);
            push_vecs(&mut v, vecs);
            v.extend_from_slice(g);
        }
        OpIn::Intersect {
            c1,
            ivl1,
            c2,
            ivl2,
            tol,
            g,
        } => {
            curve_block(&mut v, c1)?;
            v.extend_from_slice(ivl1);
            curve_block(&mut v, c2)?;
            v.extend_from_slice(ivl2);
            v.push(*tol);
            v.extend_from_slice(g);
        }
        OpIn::PropertyLocal {
            curve,
            cache,
            ivl,
            prop,
            val,
            vecs,
            sop,
            tol,
            best,
            leaf,
        } => {
            curve_block(&mut v, curve)?;
            v.extend_from_slice(&cache.ivl);
            v.extend([b(cache.full), cache.chord, cache.angle, cache.offaxis]);
            v.extend([cache.minseg as f64, b(cache.aux), cache.max3d, cache.ratio]);
            v.extend_from_slice(ivl);
            v.extend([f64::from(*prop), 1.0, *val]);
            push_vecs(&mut v, vecs);
            v.extend([f64::from(*sop), *tol, *best, *leaf as f64]);
        }
        OpIn::IsLinear { curve, tol, want } => {
            curve_block(&mut v, curve)?;
            v.extend([*tol, b(*want)]);
        }
    }
    Ok((op_of(op), v))
}

fn to_output(flat: &[f64]) -> Output {
    let status = flat.first().copied().unwrap_or(f64::NAN);
    let rest = flat.get(1..).unwrap_or(&[]).to_vec();
    Output::new()
        .ints("status", vec![status as i64])
        .values("out", rest)
}

fn cpp(op: &OpIn) -> Result<Output, HarnessError> {
    let (code, input) = cpp_input(op)?;
    match ffi::eval(code, &input) {
        Ok(flat) => Ok(to_output(&flat)),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- Rust side --------------------------------------------------------------------

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

fn pv(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
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

/// A solution object handle: an id and, for a curve object (upstream's `SmCurve*`),
/// the curve's natural interval (read by `SmSolution::IsEqual`).
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

/// A solution record (see the ffi module docs).
fn put_sol(o: &mut Vec<f64>, s: &Solution<Obj>) {
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
        if s.num_objects > 1 {
            id(s.objects[1])
        } else {
            -1.0
        },
        s.start.solution_value,
        s.start.parameters[0],
        s.start.parameters[1],
    ]);
    if s.solution_type == SolutionType::RangeOfValues {
        o.extend([
            s.end.solution_value,
            s.end.parameters[0],
            s.end.parameters[1],
        ]);
    }
}

fn sop_of(code: i32) -> Result<SolverOperation, HarnessError> {
    SolverOperation::from_code(code).ok_or_else(|| harness("solver operation"))
}

fn ivl_of(i: [f64; 2]) -> Extent1d {
    Extent1d::from_min_max(i[0], i[1])
}

/// The curve leaves (span nodes) of `tree` in pre-order from the top node.
fn leaves(tree: &Tree<Obj>) -> Result<Vec<NodeId>, HarnessError> {
    let mut out = Vec::new();
    let mut stack: Vec<NodeId> = tree.top_node().into_iter().collect();
    while let Some(id) = stack.pop() {
        let nd = tree.node(id).map_err(|e| harness(e.to_string()))?;
        if matches!(nd.data, NodeData::Curve(CurveSpanRef::Span(_))) {
            out.push(id);
        }
        if let Some(c) = nd.child2 {
            stack.push(c);
        }
        if let Some(c) = nd.child1 {
            stack.push(c);
        }
    }
    Ok(out)
}

#[allow(clippy::too_many_lines)]
fn rust(op: &OpIn) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut o: Vec<f64> = Vec::new();
    match op {
        OpIn::Normal { curves } => {
            let cs: Vec<Curve> = curves.iter().map(build).collect::<Result<_, _>>()?;
            let n = tri!(o, compute_crvs_normal(&cs));
            o.push(0.0);
            pv(&mut o, n);
        }
        OpIn::FixGaps { curves, tol } => {
            let cs: Vec<Curve> = curves.iter().map(build).collect::<Result<_, _>>()?;
            let r = tri!(o, crvs_fix_gaps(&Kernel::default(), cs, *tol));
            o.extend([0.0, r.fixed as f64, r.chained.curves.len() as f64]);
            for (c, origin) in r.chained.curves.iter().zip(&r.chained.origin) {
                o.push(*origin as f64);
                let CurveKind::BSpline(bs) = c.kind() else {
                    return Err(harness("not a B-spline"));
                };
                let nurb = bs.gw_nurb().ok_or_else(|| harness("no NURB"))?;
                o.push(nurb.cpts().len() as f64);
                for p in nurb.cpts() {
                    o.extend([p.x, p.y, p.z, p.w]);
                }
            }
        }
        OpIn::ArcLength { curve, start, len } => {
            let c = build(curve)?;
            let r = tri!(o, c.find_parameter_at_arc_length(&k, *start, *len));
            o.extend([f64::from(r.status()), r.end_param, b(r.past_end)]);
        }
        OpIn::PcExtrema {
            curve,
            p,
            dom,
            sqtol,
            sop,
            ts,
        } => {
            let c = build(curve)?;
            let mut efo = FindPcExtremaEfo::new(&c, v3(*p), ivl_of(*dom), *sqtol, sop_of(*sop)?);
            o.push(0.0);
            for &t in ts {
                let mut fv = FunctionValue::default();
                let r = efo.evaluate(t, &mut fv, true);
                let st = match st_of(&r) {
                    Ok(s) => s,
                    Err(d) => return Ok(*d),
                };
                o.extend([st, fv.f, fv.f_prime, b(fv.found_answer)]);
            }
        }
        OpIn::LineFit { c1, c2, x, jac } => {
            let a = build(c1)?;
            let bb = build(c2)?;
            let mut enfo = BestLineFitEnfo::new(&a, &bb, 1e-20);
            let mut f = [0.0; 2];
            let mut m = Matrix::new(2, 2);
            let r = enfo.evaluate(x, &mut f, if *jac { Some(&mut m) } else { None });
            let st = match st_of(&r) {
                Ok(s) => s,
                Err(d) => return Ok(*d),
            };
            let found = r.unwrap_or(false);
            o.extend([st, f[0], f[1]]);
            for i in 0..2 {
                for j in 0..2 {
                    o.push(m.at(i, j).map_err(|e| harness(e.to_string()))?);
                }
            }
            o.extend([b(found), enfo.solution_size(), enfo.scaled_zero()]);
        }
        OpIn::PointSolve {
            curve,
            ivl,
            sop,
            p,
            dist_tol,
            target,
            vecs,
            guess,
        } => {
            let c = build(curve)?;
            let vs: Vec<Vec3> = vecs.iter().map(|&x| v3(x)).collect();
            let opts = PointSolveOptions {
                distance_tolerance: *dist_tol,
                target_distance: *target,
                vectors: if vs.is_empty() { None } else { Some(&vs) },
            };
            let (found, sol) = tri!(
                o,
                c.local_point_solve::<Obj>(&ivl_of(*ivl), sop_of(*sop)?, v3(*p), &opts, *guess)
            );
            o.extend([0.0, b(found)]);
            put_sol(&mut o, &sol);
        }
        OpIn::CurveSolve {
            c1,
            ivl1,
            c2,
            ivl2,
            sop,
            tol,
            target,
            vecs,
            g,
        } => {
            let a = build(c1)?;
            let bb = build(c2)?;
            let vs: Vec<Vec3> = vecs.iter().map(|&x| v3(x)).collect();
            let (found, sol) = tri!(
                o,
                a.local_curve_solve(
                    &ivl_of(*ivl1),
                    &bb,
                    &ivl_of(*ivl2),
                    sop_of(*sop)?,
                    *tol,
                    *target,
                    if vs.is_empty() { None } else { Some(&vs) },
                    *g,
                    [Some(Obj(0, None)), Some(Obj(1, None))],
                )
            );
            o.extend([0.0, b(found)]);
            put_sol(&mut o, &sol);
        }
        OpIn::Intersect {
            c1,
            ivl1,
            c2,
            ivl2,
            tol,
            g,
        } => {
            let a = build(c1)?;
            let bb = build(c2)?;
            let r = tri!(
                o,
                local_curve_intersect(&a, &ivl_of(*ivl1), &bb, &ivl_of(*ivl2), *tol, g[0], g[1])
            );
            o.extend([0.0, b(r.is_some())]);
            if let Some(i) = r {
                o.extend([i.my_param, i.other_param, i.deviation]);
            }
        }
        OpIn::PropertyLocal {
            curve,
            cache,
            ivl,
            prop,
            val,
            vecs,
            sop,
            tol,
            best,
            leaf,
        } => {
            let c = build(curve)?;
            let opts = CurveCacheOptions {
                make_full_tree: cache.full,
                chord_height_tolerance: cache.chord,
                angle_tol_deg: cache.angle,
                off_axis_tolerance: cache.offaxis,
                min_segments: cache.minseg,
                compute_aux_tree_data: cache.aux,
                max_3d_distance: cache.max3d,
                min_parametric_ratio: cache.ratio,
            };
            let mut cc: CurveCache<Obj> = CurveCache::new(c.clone(), ivl_of(cache.ivl), opts);
            cc.set_curve_handle(Some(Obj(0, Some(c.natural_interval()))));
            tri!(o, cc.tessellate(&k));
            o.push(0.0);
            let tree = cc.tree().ok_or_else(|| harness("no tree"))?;
            let ls = leaves(tree)?;
            o.push(ls.len() as f64);
            if let Some(&nd) = ls.get((*leaf as usize) % ls.len().max(1)) {
                let prop = CurvePropertyType::from_code(*prop)
                    .ok_or_else(|| harness("property"))?;
                let vs: Vec<Vec3> = vecs.iter().map(|&x| v3(x)).collect();
                let mut g = CurvePropertyGs::new(tree, ivl_of(*ivl), prop, Some(*val), vs)
                    .with_curve(&c);
                g.base.operation = sop_of(*sop)?;
                g.base.tolerance_3d = *tol;
                g.base.best_answer_so_far_sq = *best;
                let branch: Branch = [Some(nd), None, None];
                match g.local_solve(&branch) {
                    Ok(needs) => o.extend([0.0, b(needs)]),
                    Err(e) => match Fail::from(e) {
                        Fail::Deferred(d) => return Ok(*d),
                        Fail::Status(s) => o.extend([s, 0.0]),
                    },
                }
                let sols = g.base.solutions.as_slice();
                o.push(sols.len() as f64);
                for s in sols {
                    put_sol(&mut o, s);
                }
            }
        }
        OpIn::IsLinear { curve, tol, want } => {
            let c = build(curve)?;
            let r = tri!(o, c.is_linear(*tol, *want));
            o.extend([0.0, b(r.linear)]);
            if r.linear {
                pv(&mut o, r.line_point);
                pv(&mut o, r.line_tangent);
            }
            o.push(r.deviation);
        }
    }
    Ok(to_output(&o))
}

// ---- generation -------------------------------------------------------------------

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

/// A segment curve from `p` to `q` of a random kind (Hermite, line, B-spline).
fn gen_segment(rng: &mut Rng, p: [f64; 3], q: [f64; 3]) -> CurveIn {
    let d = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
    match rng.below(3) {
        0 => {
            let bend = rp(rng, 1.0);
            CurveIn::Hermite(HermiteIn {
                dim: 3,
                p1: p,
                d1: [d[0] + bend[0], d[1] + bend[1], d[2] + bend[2]],
                p2: q,
                d2: d,
            })
        }
        1 => CurveIn::Line(LineIn {
            p,
            v: d,
            domain: [0.0, (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()],
            scale: 1.0,
            inside_out: rng.chance(0.3),
        }),
        _ => {
            let deg = 1 + rng.below(3) as usize;
            let mut cpts = vec![[p[0], p[1], p[2], NOW]];
            for i in 1..deg {
                let f = i as f64 / deg as f64;
                let j = rp(rng, 0.5);
                cpts.push([
                    p[0] + f * d[0] + j[0],
                    p[1] + f * d[1] + j[1],
                    p[2] + f * d[2] + j[2],
                    NOW,
                ]);
            }
            cpts.push([q[0], q[1], q[2], NOW]);
            let a = rng.uniform(-2.0, 2.0);
            let len = rng.uniform(0.5, 3.0);
            let mut u = vec![a; deg + 1];
            u.extend(vec![a + len; deg + 1]);
            CurveIn::Bspline(BSplineIn {
                dim: 3,
                oob: false,
                cpts,
                p: deg,
                u,
            })
        }
    }
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

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// A B-spline segment from `p` to `q` (degree 1 to 3, interior points jittered).
fn gen_bspline_segment(rng: &mut Rng, p: [f64; 3], q: [f64; 3], jitter: f64) -> BSplineIn {
    let d = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
    let deg = 1 + rng.below(3) as usize;
    let mut cpts = vec![[p[0], p[1], p[2], NOW]];
    for i in 1..deg {
        let f = i as f64 / deg as f64;
        let j = rp(rng, jitter);
        cpts.push([
            p[0] + f * d[0] + j[0],
            p[1] + f * d[1] + j[1],
            p[2] + f * d[2] + j[2],
            NOW,
        ]);
    }
    cpts.push([q[0], q[1], q[2], NOW]);
    let a = if rng.chance(0.5) {
        0.0
    } else {
        rng.uniform(-2.0, 2.0)
    };
    let len = if rng.chance(0.5) {
        1.0
    } else {
        rng.uniform(0.5, 3.0)
    };
    let mut u = vec![a; deg + 1];
    u.extend(vec![a + len; deg + 1]);
    BSplineIn {
        dim: 3,
        oob: false,
        cpts,
        p: deg,
        u,
    }
}

/// Corner `i` (cyclic) of a polygon.
fn corner(pts: &[[f64; 3]], i: usize) -> [f64; 3] {
    pts.get(i % pts.len().max(1)).copied().unwrap_or([0.0; 3])
}

/// Corners of a planar polygon (in a random plane, sometimes the XY plane).
fn gen_polygon(rng: &mut Rng, k: usize) -> Vec<[f64; 3]> {
    let c = rp(rng, 5.0);
    let xy = rng.chance(0.4);
    let x = if xy { [1.0, 0.0, 0.0] } else { gen_dir(rng) };
    let y0 = if xy { [0.0, 1.0, 0.0] } else { gen_dir(rng) };
    let n = cross(x, y0);
    let y = cross(n, x);
    let r = rng.uniform(0.5, 8.0);
    let ccw = rng.chance(0.5);
    (0..k)
        .map(|i| {
            let mut a = 2.0 * std::f64::consts::PI * i as f64 / k as f64;
            if !ccw {
                a = -a;
            }
            let rr = r * rng.uniform(0.7, 1.3);
            add(c, add(scale3(x, rr * a.cos()), scale3(y, rr * a.sin())))
        })
        .collect()
}

fn gen_normal(rng: &mut Rng) -> OpIn {
    let curves = match rng.below(5) {
        0 => vec![gen_curve(rng)],
        4 => vec![gen_closed_curve(rng)],
        1 => (0..=rng.below(4)).map(|_| gen_curve(rng)).collect(),
        _ => {
            let k = 2 + rng.below(5) as usize;
            let pts = gen_polygon(rng, k);
            let mut cs: Vec<CurveIn> = (0..k)
                .map(|i| {
                    let p = corner(&pts, i);
                    let q = corner(&pts, i + 1);
                    gen_segment(rng, p, q)
                })
                .collect();
            if rng.chance(0.2) {
                cs.pop();
            }
            if rng.chance(0.1) {
                cs.truncate(1);
            }
            cs
        }
    };
    OpIn::Normal { curves }
}

/// A chain of B-spline segments with small gaps, sometimes closed, shuffled and with
/// some curves reversed.
fn gen_fix_gaps(rng: &mut Rng) -> OpIn {
    let k = 1 + rng.below(6) as usize;
    let closed = rng.chance(0.6);
    let pts = gen_polygon(rng, k + usize::from(!closed));
    let gaps = [0.0, 1e-13, 1e-9, 1e-6, 1e-4, 1e-3, 1e-2, 0.2];
    let mut curves: Vec<CurveIn> = (0..k)
        .map(|i| {
            let p = corner(&pts, i);
            let mut q = corner(&pts, i + 1);
            let g = *rng.choose(&gaps).unwrap_or(&0.0);
            let d = gen_dir(rng);
            q = add(q, scale3(d, g));
            let mut bs = gen_bspline_segment(rng, p, q, 0.5);
            if rng.chance(0.25) {
                bs.cpts.reverse();
            }
            CurveIn::Bspline(bs)
        })
        .collect();
    if k == 1 && rng.chance(0.5) {
        // a single nearly closed curve
        let p = corner(&pts, 0);
        let mid = corner(&pts, pts.len() / 2);
        let g = *rng.choose(&gaps).unwrap_or(&0.0);
        let q = add(p, [g, 0.0, 0.0]);
        let mut bs = gen_bspline_segment(rng, p, q, 0.0);
        bs.p = 2;
        bs.cpts = vec![[p[0], p[1], p[2], NOW], [mid[0], mid[1], mid[2] + 1.0, NOW], [q[0], q[1], q[2], NOW]];
        let a = bs.u.first().copied().unwrap_or(0.0);
        let e = bs.u.last().copied().unwrap_or(1.0);
        bs.u = vec![a, a, a, e, e, e];
        curves = vec![CurveIn::Bspline(bs)];
    }
    // shuffle
    for i in (1..curves.len()).rev() {
        let j = rng.below(i as u64 + 1) as usize;
        curves.swap(i, j);
    }
    let tol = *rng.choose(&[1e-4, 1e-3, 1e-2, 0.5]).unwrap_or(&1e-3);
    OpIn::FixGaps { curves, tol }
}

fn gen_arc_length(rng: &mut Rng) -> OpIn {
    let curve = if rng.chance(0.6) {
        CurveIn::Line(gen_line(rng))
    } else {
        gen_curve(rng)
    };
    let (a, bb) = range_of(&curve);
    let start = match rng.below(6) {
        0 => a,
        1 => bb,
        2 => a - (bb - a) * 1e-12,
        _ => a + (bb - a) * rng.unit(),
    };
    let total = (bb - a).abs().max(1e-3) * 5.0;
    let mut len = match rng.below(8) {
        0 => 0.0,
        1 => total * 100.0,
        2 => 1e-9,
        _ => total * rng.uniform(-1.0, 1.0),
    };
    if len < 0.0 && start < a {
        len = -len;
    }
    OpIn::ArcLength {
        curve,
        start,
        len,
    }
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
fn gen_point_near(rng: &mut Rng, c: &CurveIn) -> [f64; 3] {
    let Ok(curve) = build(c) else {
        return rp(rng, 10.0);
    };
    let t = gen_t(rng, c);
    let base = curve.evaluate_point(t).map_or([0.0; 3], |p| [p.x, p.y, p.z]);
    match rng.below(5) {
        0 => base,
        1 => rp(rng, 20.0),
        _ => {
            let r = rng.uniform(0.01, 5.0);
            add(base, rp(rng, r))
        }
    }
}

fn gen_pc_extrema(rng: &mut Rng) -> OpIn {
    let curve = gen_curve_or_degenerate(rng);
    let p = gen_point_near(rng, &curve);
    let dom = gen_ivl(rng, &curve);
    let sqtol = *rng.choose(&[1e-20, 1e-12, 1e-6, 1e-2]).unwrap_or(&1e-20);
    let sop = *rng.choose(&[3, 4, 5, 3]).unwrap_or(&3);
    let n = 1 + rng.below(5) as usize;
    let ts = (0..n).map(|_| gen_t(rng, &curve)).collect();
    OpIn::PcExtrema {
        curve,
        p,
        dom,
        sqtol,
        sop,
        ts,
    }
}

fn gen_line_fit(rng: &mut Rng) -> OpIn {
    let c1 = gen_curve_or_degenerate(rng);
    let c2 = if rng.chance(0.3) {
        // a nearby copy: tangent (or nearly) at equal parameters
        match &c1 {
            CurveIn::Hermite(h) => {
                let o = scale3(gen_dir(rng), *rng.choose(&[0.0, 1e-3, 0.05]).unwrap_or(&0.0));
                CurveIn::Hermite(HermiteIn {
                    p1: add(h.p1, o),
                    p2: add(h.p2, o),
                    ..h.clone()
                })
            }
            other => other.clone(),
        }
    } else {
        gen_curve_or_degenerate(rng)
    };
    let x0 = gen_t(rng, &c1);
    let x1 = if rng.chance(0.3) { x0 } else { gen_t(rng, &c2) };
    OpIn::LineFit {
        c1,
        c2,
        x: [x0, x1],
        jac: rng.chance(0.7),
    }
}

fn gen_point_solve(rng: &mut Rng) -> OpIn {
    let curve = gen_curve_or_degenerate(rng);
    let ivl = gen_ivl(rng, &curve);
    let sop = *rng
        .choose(&[3, 3, 3, 4, 4, 5, 5, 0, 6, 6, 8, 9])
        .unwrap_or(&3);
    let p = gen_point_near(rng, &curve);
    let dist_tol = if matches!(sop, 8 | 9) || rng.chance(0.2) {
        Some(*rng.choose(&[1e-6, 1e-3, 1.0]).unwrap_or(&1e-3))
    } else {
        None
    };
    let target = if sop == 6 || rng.chance(0.2) {
        Some(rng.uniform(0.0, 10.0))
    } else {
        None
    };
    let vecs = if matches!(sop, 8 | 9) {
        vec![gen_dir(rng)]
    } else {
        Vec::new()
    };
    let guess = gen_t(rng, &curve);
    OpIn::PointSolve {
        curve,
        ivl,
        sop,
        p,
        dist_tol,
        target,
        vecs,
        guess,
    }
}

fn gen_curve_solve(rng: &mut Rng) -> OpIn {
    let c1 = gen_curve(rng);
    let c2 = gen_curve(rng);
    let ivl1 = gen_ivl(rng, &c1);
    let ivl2 = gen_ivl(rng, &c2);
    let sop = *rng
        .choose(&[3, 4, 5, 6, 9, 11, 12, 13, 14, 15, 23])
        .unwrap_or(&3);
    let need = match sop {
        9 | 14 | 15 => 1,
        11..=13 => 2,
        23 => 3,
        _ => 0,
    };
    let vecs = (0..need).map(|_| gen_dir(rng)).collect();
    let g = [gen_t(rng, &c1), gen_t(rng, &c2)];
    OpIn::CurveSolve {
        c1,
        ivl1,
        c2,
        ivl2,
        sop,
        tol: *rng.choose(&[1e-6, 1e-3]).unwrap_or(&1e-6),
        target: if sop == 6 || rng.chance(0.2) {
            Some(rng.uniform(0.0, 5.0))
        } else {
            None
        },
        vecs,
        g,
    }
}

fn gen_intersect(rng: &mut Rng) -> OpIn {
    let c1 = gen_curve(rng);
    let c2 = gen_curve(rng);
    let ivl1 = gen_ivl(rng, &c1);
    let ivl2 = gen_ivl(rng, &c2);
    let g = [gen_t(rng, &c1), gen_t(rng, &c2)];
    OpIn::Intersect {
        c1,
        ivl1,
        c2,
        ivl2,
        tol: *rng.choose(&[1e-6, 1e-3, 0.1]).unwrap_or(&1e-6),
        g,
    }
}

/// A planar B-spline or Hermite curve in the plane `z = z0`.
fn gen_planar(rng: &mut Rng, z0: f64) -> CurveIn {
    match gen_curve(rng) {
        CurveIn::Bspline(mut bs) => {
            for c in &mut bs.cpts {
                if c[2] != NOZ {
                    c[2] = if c[3] == NOW { z0 } else { z0 * c[3] };
                }
            }
            CurveIn::Bspline(bs)
        }
        CurveIn::Hermite(mut h) => {
            h.p1[2] = z0;
            h.p2[2] = z0;
            h.d1[2] = 0.0;
            h.d2[2] = 0.0;
            CurveIn::Hermite(h)
        }
        _ => CurveIn::Hermite(gen_hermite(rng)),
    }
}

fn gen_property_local(rng: &mut Rng) -> OpIn {
    // properties: angles (17-19), plane intersection (16), and a few that reach the
    // property analysis
    let prop = *rng
        .choose(&[17, 18, 19, 17, 18, 19, 16, 16, 16, 16, 0, 4, 11, 20, 23])
        .unwrap_or(&16);
    let z0 = rng.uniform(-2.0, 2.0);
    let planar = prop == 16 && rng.chance(0.5);
    let curve = if planar {
        gen_planar(rng, z0)
    } else {
        match gen_curve(rng) {
            CurveIn::Line(_) => CurveIn::Hermite(gen_hermite(rng)),
            c => c,
        }
    };
    let (a, bb) = range_of(&curve);
    let cache = CacheIn {
        ivl: [a, bb],
        full: rng.chance(0.3),
        chord: if rng.chance(0.6) {
            rng.uniform(1e-3, 0.5)
        } else {
            0.0
        },
        angle: if rng.chance(0.6) {
            rng.uniform(5.0, 60.0)
        } else {
            0.0
        },
        offaxis: 0.0,
        minseg: if rng.chance(0.4) { 1 + rng.below(6) } else { 0 },
        aux: rng.chance(0.3),
        max3d: 0.0,
        ratio: 1e-4,
    };
    let mut cache = cache;
    if cache.chord == 0.0 && cache.angle == 0.0 && rng.chance(0.95) {
        cache.angle = 20.0;
    }
    let ivl = if rng.chance(0.6) {
        [a, bb]
    } else {
        gen_ivl(rng, &curve)
    };
    let (val, vecs) = if prop == 16 {
        // plane z = z0 (or a random plane through a curve point)
        if rng.chance(0.6) {
            (-z0, vec![[0.0, 0.0, 1.0], gen_dir(rng), gen_dir(rng)])
        } else {
            let n = gen_dir(rng);
            let p = gen_point_near(rng, &curve);
            let d = -(n[0] * p[0] + n[1] * p[1] + n[2] * p[2]);
            (d, vec![n, gen_dir(rng), gen_dir(rng)])
        }
    } else {
        (rng.uniform(0.0, 5.0), vec![rp(rng, 10.0), gen_dir(rng), gen_dir(rng)])
    };
    let best = *rng
        .choose(&[0.0, -1.0, 1e-3, 0.5, 10.0, 1e3])
        .unwrap_or(&0.0);
    OpIn::PropertyLocal {
        curve,
        cache,
        ivl,
        prop,
        val,
        vecs,
        sop: *rng.choose(&[0, 3, 4]).unwrap_or(&0),
        tol: *rng.choose(&[1e-6, 1e-3, 0.1]).unwrap_or(&1e-3),
        best,
        // the first (pre-order) leaf starts the solver interval: a discontinuity end
        leaf: if planar && rng.chance(0.6) {
            0
        } else {
            rng.below(64)
        },
    }
}

/// A straight B-spline (collinear control points), sometimes doubling back.
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

fn gen_is_linear(rng: &mut Rng) -> OpIn {
    let curve = match rng.below(4) {
        0 | 1 => gen_straight(rng),
        2 => CurveIn::Line(gen_line(rng)),
        _ => gen_curve_or_degenerate(rng),
    };
    OpIn::IsLinear {
        curve,
        tol: *rng.choose(&[1e-10, 1e-6, 1e-3, 0.1]).unwrap_or(&1e-6),
        want: rng.chance(0.4),
    }
}

fn gen_case(rng: &mut Rng) -> OpIn {
    match rng.below(100) {
        0..=9 => gen_normal(rng),
        10..=19 => gen_fix_gaps(rng),
        20..=31 => gen_arc_length(rng),
        32..=43 => gen_pc_extrema(rng),
        44..=51 => gen_line_fit(rng),
        52..=67 => gen_point_solve(rng),
        68..=71 => gen_curve_solve(rng),
        72..=75 => gen_intersect(rng),
        76..=91 => gen_property_local(rng),
        _ => gen_is_linear(rng),
    }
}

/// True when `CrvsFixGaps` would delete a duplicate curve (a use after free upstream).
fn deletes_duplicates(op: &OpIn) -> bool {
    let OpIn::FixGaps { curves, tol } = op else {
        return false;
    };
    let Ok(list) = curves
        .iter()
        .map(|c| build(c).map(Some))
        .collect::<Result<Vec<_>, _>>()
    else {
        return true;
    };
    fix_chain_curves_list(&Kernel::default(), list, *tol, true).map_or(true, |r| r.deleted > 0)
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.curve_p3"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let op = gen_case(rng);
            let Ok((code, input)) = cpp_input(&op) else {
                continue;
            };
            if ffi::validate(code, &input).is_err() || deletes_duplicates(&op) {
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
        rust(&case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Generated cases cover every op, with passing (not deferred) cases for the ops
    /// that can be ported now.
    #[test]
    fn generated_cases_are_diverse() {
        let mut rng = Rng::new(7);
        let mut stats: BTreeMap<String, [usize; 4]> = BTreeMap::new();
        for _ in 0..2000 {
            let op = gen_case(&mut rng);
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
        assert_eq!(stats.len(), 10);
    }
}

