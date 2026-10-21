//! Runner for unit `curve.curve_p4`: `SmCurve` part 4 (closed / degenerate tests, polar
//! box, knot snapping, `AssertValid`, plane crossings, chaining, the analytic
//! intersection dispatcher), the solver helpers `sm_FilterSolution` /
//! `sm_GetProjDist` and the curve evaluator objects.
//!
//! C++: `solidcore_ffi::units::curve_curve_p4` (upstream `source/SMLib/src/SmCurve.cpp`,
//! called through `SmCurve*` on B-spline, Hermite and line curves; the file-static
//! helpers and local classes are verbatim copies in the shim). Rust:
//! `solidcore::curve::curve::p4`.
//!
//! A case is one `op` with its curves (`{"bspline": {dim, oob, cpts, p, u}}` with
//! control points as stored `[x, y, z, w]`, `{"hermite": {dim, p1, d1, p2, d2}}`,
//! `{"circle": ..}` (a quadratic B-spline arc) or `{"line": {p, v, domain, scale,
//! inside_out}}`) and parameters. Both sides produce the flat record documented in the
//! ffi module; the runner reports its first entry as the `status` int (`0` =
//! `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a Rust
//! error upstream cannot produce) and the rest as the `out` values.
//!
//! Deferred cases: `HasPlaneCrossings` with planes (`SmSurface::GlobalCurveIntersect`,
//! `surface.surface_p8`) and ellipse intersections that reach the general curve solvers
//! (`curve.curve_p1`, `curve.curve_p3`, `curve.curve_p6`). `FixChainCurvesList` cases
//! that would delete a duplicate are not generated (a use after free upstream, see
//! `plan/findings.md`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::{Curve, CurvePropertyType, FindCurveDistanceExtrema};
use solidcore::curve::curve::p4::{
    analytic_curve_intersect, approximate_polar_box, assert_valid_base, filter_solution,
    fix_chain_curves_list, has_plane_crossings, is_closed_base, is_degenerate_base, proj_dist,
    AnalyticOperand, CcTangentIntEnfo, CpDistAtEnfo, CurvePropertyEfo,
};
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::assert_array::{AssertArray, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::geom::global_solver::SolverOperation;
use solidcore::geom::local_solve1d::{EvalFunction, FunctionValue};
use solidcore::geom::local_solve_nd::EvalNFunctions;
use solidcore::geom::matrix::Matrix;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::solution_array::{ObjectKind, Solution, SolutionObject, SolutionType};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::surface::plane::Plane;
use solidcore::SolidError;
use solidcore_ffi::units::curve_curve_p4::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `curve.curve_p4` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// `SmHermiteCurve_TYPE`.
const HERMITE_CURVE_TYPE: i64 = 14034;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EllipseIn {
    c: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    domain: [f64; 2],
    radii: [f64; 2],
    #[serde(default)]
    inside_out: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OperandIn {
    Line(LineIn),
    Ellipse(EllipseIn),
    Curve(CurveIn),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    IsClosed {
        curve: CurveIn,
        base: bool,
        ivl: [f64; 2],
        tol: f64,
    },
    IsDegenerate {
        curve: CurveIn,
        base: bool,
        tol: f64,
        ivl: Option<[f64; 2]>,
    },
    PolarBox {
        curve: CurveIn,
        ivl: [f64; 2],
        count: u64,
    },
    Snap {
        curve: CurveIn,
        t: f64,
        tol: f64,
    },
    Assert {
        curve: CurveIn,
        owner: bool,
        given: bool,
    },
    FixChain {
        curves: Vec<Option<CurveIn>>,
        tol: f64,
        del: bool,
    },
    Filter {
        c1: CurveIn,
        ivl1: [f64; 2],
        c2: CurveIn,
        ivl2: [f64; 2],
        k1: [u8; 2],
        k2: [u8; 2],
        adj: u64,
        s: [f64; 2],
        value: f64,
    },
    ProjDist {
        curve: CurveIn,
        t: f64,
        p: [f64; 3],
        n: [f64; 3],
        rev: bool,
        d: [f64; 3],
        sop: i32,
    },
    Property {
        curve: CurveIn,
        prop: i32,
        val: Option<f64>,
        vecs: Vec<[f64; 3]>,
        sqtol: f64,
        t: f64,
        signal: bool,
    },
    CcTangent {
        c1: CurveIn,
        c2: CurveIn,
        x: [f64; 2],
        jac: bool,
    },
    CpDistAt {
        curve: CurveIn,
        p: [f64; 3],
        dsq: f64,
        sqtol: f64,
        x: [f64; 4],
        jac: bool,
    },
    DistExtrema {
        c1: CurveIn,
        c2: CurveIn,
        dom: [f64; 2],
        tol: f64,
        t: f64,
    },
    Analytic {
        a: OperandIn,
        fa: [f64; 2],
        b: OperandIn,
        fb: [f64; 2],
        tol: f64,
    },
    Planes {
        curve: CurveIn,
        planes: Vec<[[f64; 3]; 2]>,
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

fn operand_block(v: &mut Vec<f64>, o: &OperandIn) -> Result<(), HarnessError> {
    match o {
        OperandIn::Line(l) => {
            v.push(0.0);
            push_line(v, l);
        }
        OperandIn::Ellipse(e) => {
            v.push(1.0);
            v.extend_from_slice(&e.c);
            v.extend_from_slice(&e.x);
            v.extend_from_slice(&e.y);
            v.extend([e.domain[0], e.domain[1], e.radii[0], e.radii[1], 3.0, b(e.inside_out)]);
        }
        OperandIn::Curve(c) => {
            v.push(2.0);
            curve_block(v, c)?;
        }
    }
    Ok(())
}

fn op_of(op: &OpIn) -> Op {
    match op {
        OpIn::IsClosed { .. } => Op::IsClosed,
        OpIn::IsDegenerate { .. } => Op::IsDegenerate,
        OpIn::PolarBox { .. } => Op::PolarBox,
        OpIn::Snap { .. } => Op::Snap,
        OpIn::Assert { .. } => Op::Assert,
        OpIn::FixChain { .. } => Op::FixChain,
        OpIn::Filter { .. } => Op::Filter,
        OpIn::ProjDist { .. } => Op::ProjDist,
        OpIn::Property { .. } => Op::Property,
        OpIn::CcTangent { .. } => Op::CcTangent,
        OpIn::CpDistAt { .. } => Op::CpDistAt,
        OpIn::DistExtrema { .. } => Op::DistExtrema,
        OpIn::Analytic { .. } => Op::Analytic,
        OpIn::Planes { .. } => Op::Planes,
    }
}

#[allow(clippy::too_many_lines)]
fn cpp_input(op: &OpIn) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut v = Vec::new();
    match op {
        OpIn::IsClosed {
            curve,
            base,
            ivl,
            tol,
        } => {
            curve_block(&mut v, curve)?;
            v.extend([b(*base), ivl[0], ivl[1], *tol]);
        }
        OpIn::IsDegenerate {
            curve,
            base,
            tol,
            ivl,
        } => {
            curve_block(&mut v, curve)?;
            let [a, bb] = ivl.unwrap_or([0.0, 0.0]);
            let (a, bb) = if ivl.is_some() {
                (a, bb)
            } else {
                let r = range_of(curve);
                (r.0, r.1)
            };
            v.extend([b(*base), *tol, b(ivl.is_some()), a, bb]);
        }
        OpIn::PolarBox { curve, ivl, count } => {
            curve_block(&mut v, curve)?;
            v.extend([ivl[0], ivl[1], *count as f64]);
        }
        OpIn::Snap { curve, t, tol } => {
            curve_block(&mut v, curve)?;
            v.extend([*t, *tol]);
        }
        OpIn::Assert {
            curve,
            owner,
            given,
        } => {
            curve_block(&mut v, curve)?;
            v.extend([b(*owner), b(*given)]);
        }
        OpIn::FixChain { curves, tol, del } => {
            v.push(curves.len() as f64);
            for c in curves {
                v.push(b(c.is_some()));
                if let Some(c) = c {
                    curve_block(&mut v, c)?;
                }
            }
            v.extend([*tol, b(*del)]);
        }
        OpIn::Filter {
            c1,
            ivl1,
            c2,
            ivl2,
            k1,
            k2,
            adj,
            s,
            value,
        } => {
            curve_block(&mut v, c1)?;
            v.extend_from_slice(ivl1);
            curve_block(&mut v, c2)?;
            v.extend_from_slice(ivl2);
            v.extend([k1[0], k1[1], k2[0], k2[1]].map(f64::from));
            v.extend([*adj as f64, s[0], s[1], *value]);
        }
        OpIn::ProjDist {
            curve,
            t,
            p,
            n,
            rev,
            d,
            sop,
        } => {
            curve_block(&mut v, curve)?;
            v.push(*t);
            v.extend_from_slice(p);
            v.extend_from_slice(n);
            v.push(b(*rev));
            v.extend_from_slice(d);
            v.push(f64::from(*sop));
        }
        OpIn::Property {
            curve,
            prop,
            val,
            vecs,
            sqtol,
            t,
            signal,
        } => {
            curve_block(&mut v, curve)?;
            v.extend([f64::from(*prop), b(val.is_some()), val.unwrap_or(0.0)]);
            v.push(vecs.len() as f64);
            for x in vecs {
                v.extend_from_slice(x);
            }
            v.extend([*sqtol, *t, b(*signal)]);
        }
        OpIn::CcTangent { c1, c2, x, jac } => {
            curve_block(&mut v, c1)?;
            curve_block(&mut v, c2)?;
            v.extend([x[0], x[1], b(*jac)]);
        }
        OpIn::CpDistAt {
            curve,
            p,
            dsq,
            sqtol,
            x,
            jac,
        } => {
            curve_block(&mut v, curve)?;
            v.extend_from_slice(p);
            v.extend([*dsq, *sqtol]);
            v.extend_from_slice(x);
            v.push(b(*jac));
        }
        OpIn::DistExtrema {
            c1,
            c2,
            dom,
            tol,
            t,
        } => {
            curve_block(&mut v, c1)?;
            curve_block(&mut v, c2)?;
            v.extend([dom[0], dom[1], *tol, *t]);
        }
        OpIn::Analytic { a, fa, b: ob, fb, tol } => {
            operand_block(&mut v, a)?;
            v.extend_from_slice(fa);
            operand_block(&mut v, ob)?;
            v.extend_from_slice(fb);
            v.push(*tol);
        }
        OpIn::Planes { curve, planes } => {
            curve_block(&mut v, curve)?;
            v.push(planes.len() as f64);
            for [p, n] in planes {
                v.extend_from_slice(p);
                v.extend_from_slice(n);
            }
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

fn build_ellipse(k: &Kernel, e: &EllipseIn) -> solidcore::Result<Ellipse> {
    let opts = EllipseOptions {
        dim: 3,
        nurb: None,
        inside_out: e.inside_out,
    };
    Ellipse::new(
        k,
        v3(e.c),
        v3(e.x),
        v3(e.y),
        Extent1d::from_min_max(e.domain[0], e.domain[1]),
        e.radii[0],
        e.radii[1],
        &opts,
    )
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

#[derive(Debug, Clone, Copy, PartialEq)]
struct Obj(i64);

impl SolutionObject for Obj {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Other
    }
}

const CONTINUITIES: [ContinuityType; 14] = [
    ContinuityType::Undefined,
    ContinuityType::Discontinuous,
    ContinuityType::C0,
    ContinuityType::G1,
    ContinuityType::G1R,
    ContinuityType::G1G2,
    ContinuityType::G1G2G3,
    ContinuityType::C1,
    ContinuityType::C1G2,
    ContinuityType::C1G2G3,
    ContinuityType::C1C2,
    ContinuityType::C1C2G3,
    ContinuityType::C1C2C3,
    ContinuityType::CInfinity,
];

fn cont(c: u8) -> Result<ContinuityType, HarnessError> {
    CONTINUITIES
        .get(usize::from(c))
        .copied()
        .ok_or_else(|| harness("continuity code"))
}

fn put_sols(o: &mut Vec<f64>, sols: &[Solution<i64>]) {
    o.push(sols.len() as f64);
    for s in sols {
        let id = |x: Option<i64>| x.map_or(-1.0, |v| v as f64);
        o.extend([
            f64::from(s.solution_type as i32),
            s.num_variables as f64,
            s.num_objects as f64,
            id(s.objects[0]),
            if s.num_objects > 1 {
                id(s.objects[1])
            } else {
                -1.0
            },
            s.start.solution_value,
            s.start.parameters[0],
            if s.num_variables > 1 {
                s.start.parameters[1]
            } else {
                0.0
            },
        ]);
        if s.solution_type == SolutionType::RangeOfValues {
            o.extend([
                s.end.solution_value,
                s.end.parameters[0],
                if s.num_variables > 1 {
                    s.end.parameters[1]
                } else {
                    0.0
                },
            ]);
        }
    }
}

/// The natural interval of an operand between the fractions `f`.
enum Built {
    Curve(Curve),
    Ellipse(Ellipse),
}

fn as_op(x: &Built) -> (AnalyticOperand<'_>, Extent1d) {
    match x {
        Built::Curve(c) => (AnalyticOperand::Curve(c), c.natural_interval()),
        Built::Ellipse(e) => (AnalyticOperand::Ellipse(e), e.base().natural_interval()),
    }
}

fn sub_ivl(nat: Extent1d, f: [f64; 2]) -> Extent1d {
    Extent1d::from_min_max(nat.evaluate(f[0]), nat.evaluate(f[1]))
}

#[allow(clippy::too_many_lines)]
fn rust(k: &Kernel, op: &OpIn) -> Result<Output, HarnessError> {
    let mut o: Vec<f64> = Vec::new();
    match op {
        OpIn::IsClosed {
            curve,
            base,
            ivl,
            tol,
        } => {
            let c = build(curve)?;
            let i = Extent1d::from_min_max(ivl[0], ivl[1]);
            let r = if *base {
                is_closed_base(k, &c, &i, *tol)
            } else {
                c.is_closed(k, &i, *tol)
            };
            let r = tri!(o, r);
            o.extend([0.0, b(r)]);
        }
        OpIn::IsDegenerate {
            curve,
            base,
            tol,
            ivl,
        } => {
            let c = build(curve)?;
            let i = ivl.map(|v| Extent1d::from_min_max(v[0], v[1]));
            let r = if *base {
                is_degenerate_base(k, &c, *tol, i.as_ref())
            } else {
                c.is_degenerate(k, *tol, i.as_ref())
            };
            let r = tri!(o, r);
            o.extend([0.0, b(r)]);
        }
        OpIn::PolarBox { curve, ivl, count } => {
            // the FFI rejects more than `ffi::MAX_SAMPLES` samples; upstream (and the port)
            // would loop that many times, so a larger count is not a case for Rust either
            if *count > ffi::MAX_SAMPLES as u64 {
                return Err(HarnessError::new(format!("{count} samples")));
            }
            let c = build(curve)?;
            let bx = tri!(
                o,
                approximate_polar_box(k, &c, &Extent1d::from_min_max(ivl[0], ivl[1]), *count)
            );
            o.push(0.0);
            o.push(f64::from(bx.state().code()));
            for v in bx.basis() {
                pv(&mut o, *v);
            }
            let d = bx.domain();
            o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
        }
        OpIn::Snap { curve, t, tol } => {
            let c = build(curve)?;
            let r = tri!(o, c.snap_to_knots(k, *t, *tol));
            o.extend([0.0, r]);
        }
        OpIn::Assert {
            curve,
            owner,
            given,
        } => {
            let c = build(curve)?;
            let own = owner.then_some(ObjectRef {
                type_code: HERMITE_CURVE_TYPE,
                key: solidcore::geom::attribute::ObjectKey(99),
            });
            let mut list = AssertArray::new();
            let ok = tri!(
                o,
                assert_valid_base(
                    own,
                    c.dim(),
                    c.class_string(),
                    Some(&mut list),
                    *given,
                    ObjectKey(1)
                )
            );
            o.extend([0.0, b(ok), list.len() as f64]);
            for r in list.reports() {
                o.extend([
                    r.test_index as f64,
                    r.owner_type as f64,
                    r.tol3d,
                    r.value,
                    r.list_index as f64,
                ]);
            }
        }
        OpIn::FixChain { curves, tol, del } => {
            let mut list = Vec::new();
            for c in curves {
                list.push(match c {
                    Some(c) => Some(build(c)?),
                    None => None,
                });
            }
            let r = tri!(o, fix_chain_curves_list(&Kernel::default(), list, *tol, *del));
            if r.deleted > 0 {
                return Err(harness("a duplicate was deleted (use after free upstream)"));
            }
            o.extend([
                0.0,
                r.chain_starts.len() as f64,
                r.chain_starts.len() as f64,
            ]);
            o.extend(r.chain_starts.iter().map(|&s| s as f64));
            o.push(r.curves.len() as f64);
            for (c, &orig) in r.curves.iter().zip(&r.origin) {
                o.push(orig as f64);
                let nat = c.natural_interval();
                o.extend([nat.min(), nat.max()]);
                for t in [nat.min(), nat.max()] {
                    let p = match c.evaluate_point(k, t) {
                        Ok(p) => p,
                        Err(e) => match Fail::from(e) {
                            Fail::Deferred(d) => return Ok(*d),
                            Fail::Status(_) => Vec3::UNDEF,
                        },
                    };
                    pv(&mut o, p);
                }
            }
        }
        OpIn::Filter {
            c1,
            ivl1,
            c2,
            ivl2,
            k1,
            k2,
            adj,
            s,
            value,
        } => {
            let a = build(c1)?;
            let bb = build(c2)?;
            let mut sol = Solution::<Obj>::new();
            sol.start.parameters[0] = s[0];
            sol.start.parameters[1] = s[1];
            sol.start.solution_value = *value;
            let r = filter_solution(
                k, &a,
                &Extent1d::from_min_max(ivl1[0], ivl1[1]),
                &bb,
                &Extent1d::from_min_max(ivl2[0], ivl2[1]),
                [cont(k1[0])?, cont(k1[1])?],
                [cont(k2[0])?, cont(k2[1])?],
                *adj,
                &sol,
            );
            let keep = tri!(o, r);
            o.extend([0.0, b(keep)]);
        }
        OpIn::ProjDist {
            curve,
            t,
            p,
            n,
            rev,
            d,
            sop,
        } => {
            let c = build(curve)?;
            let sop = SolverOperation::from_code(*sop).ok_or_else(|| harness("sop"))?;
            let r = proj_dist(k, &c, *t, v3(*p), v3(*n), *rev, v3(*d), sop);
            // upstream returns a failing status as the distance
            let value = match r {
                Ok(v) => v,
                Err(e) => match Fail::from(e) {
                    Fail::Deferred(dd) => return Ok(*dd),
                    Fail::Status(s) => s,
                },
            };
            o.extend([0.0, value]);
        }
        OpIn::Property {
            curve,
            prop,
            val,
            vecs,
            sqtol,
            t,
            signal,
        } => {
            let c = build(curve)?;
            let prop = CurvePropertyType::from_code(*prop).ok_or_else(|| harness("prop"))?;
            let vs: Vec<Vec3> = vecs.iter().map(|x| v3(*x)).collect();
            let mut efo = CurvePropertyEfo::new(
                k, &c,
                prop,
                *val,
                if vs.is_empty() { None } else { Some(&vs) },
                *sqtol,
            );
            let mut out = FunctionValue::default();
            let r = efo.evaluate(*t, &mut out, *signal);
            let s = match st_of(&r) {
                Ok(s) => s,
                Err(d) => return Ok(*d),
            };
            o.extend([s, out.f, out.f_prime, b(out.found_answer)]);
        }
        OpIn::CcTangent { c1, c2, x, jac } => {
            let a = build(c1)?;
            let bb = build(c2)?;
            let mut f = [0.0; 2];
            let mut m = Matrix::new(2, 2);
            let r = CcTangentIntEnfo::static_evaluate(
                k, &a,
                &bb,
                x,
                &mut f,
                if *jac { Some(&mut m) } else { None },
            );
            let s = match st_of(&r) {
                Ok(s) => s,
                Err(d) => return Ok(*d),
            };
            o.push(s);
            o.extend_from_slice(&f);
            o.extend_from_slice(m.as_slice());
            o.push(b(r.unwrap_or(false)));
        }
        OpIn::CpDistAt {
            curve,
            p,
            dsq,
            sqtol,
            x,
            jac,
        } => {
            let c = build(curve)?;
            let mut e = CpDistAtEnfo::new(k, &c, v3(*p), *dsq, *sqtol);
            let mut f = [0.0; 4];
            let mut m = Matrix::new(4, 4);
            let r = e.evaluate(x, &mut f, if *jac { Some(&mut m) } else { None });
            let s = match st_of(&r) {
                Ok(s) => s,
                Err(d) => return Ok(*d),
            };
            o.push(s);
            o.extend_from_slice(&f);
            o.extend_from_slice(m.as_slice());
            o.push(b(r.unwrap_or(false)));
        }
        OpIn::DistExtrema {
            c1,
            c2,
            dom,
            tol,
            t,
        } => {
            let a = build(c1)?;
            let bb = build(c2)?;
            let mut efo = FindCurveDistanceExtrema::new(
                k, &a,
                &bb,
                Extent1d::from_min_max(dom[0], dom[1]),
                *tol,
            );
            let mut out = FunctionValue::default();
            let r = efo.evaluate(*t, &mut out, true);
            let s = match st_of(&r) {
                Ok(s) => s,
                Err(d) => return Ok(*d),
            };
            o.extend([s, out.f, out.f_prime, b(out.found_answer)]);
        }
        OpIn::Analytic {
            a,
            fa,
            b: ob,
            fb,
            tol,
        } => {
            let k = Kernel::default();
            let mk = |x: &OperandIn| -> Result<solidcore::Result<Built>, HarnessError> {
                Ok(match x {
                    OperandIn::Line(l) => Ok(Built::Curve(Curve::from(build_line(l)))),
                    OperandIn::Curve(c) => Ok(Built::Curve(build(c)?)),
                    OperandIn::Ellipse(e) => build_ellipse(&k, e).map(Built::Ellipse),
                })
            };
            let ba = tri!(o, mk(a)?);
            let bb = tri!(o, mk(ob)?);
            let (oa, na) = as_op(&ba);
            let (obb, nb) = as_op(&bb);
            let r = analytic_curve_intersect::<i64>(&Kernel::default(), 
                oa,
                &sub_ivl(na, *fa),
                obb,
                &sub_ivl(nb, *fb),
                *tol,
                Some(0),
                Some(1),
            );
            let r = tri!(o, r);
            o.extend([0.0, b(r.needs_more_intersections)]);
            put_sols(&mut o, &r.solutions);
        }
        OpIn::Planes { curve, planes } => {
            let c = build(curve)?;
            let planes: Vec<Plane> = planes
                .iter()
                .map(|[p, n]| Plane::from_normal(Vec3::from_array(*p), Vec3::from_array(*n)))
                .collect();
            let r = tri!(o, has_plane_crossings(&Kernel::default(), &c, &planes));
            o.extend([0.0, b(r)]);
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

fn gen_chain(rng: &mut Rng) -> OpIn {
    let mut curves: Vec<Option<CurveIn>> = Vec::new();
    let tol = *rng.choose(&[1e-6, 1e-3, 0.05, 0.5]).unwrap_or(&1e-3);
    let nchains = 1 + rng.below(2);
    for _ in 0..nchains {
        let k = 1 + rng.below(5) as usize;
        let closed = rng.chance(0.5) && k >= 3;
        let base = rp(rng, 20.0);
        let mut pts: Vec<[f64; 3]> = (0..k)
            .map(|_| {
                let o = rp(rng, 5.0);
                [base[0] + o[0], base[1] + o[1], base[2] + o[2]]
            })
            .collect();
        if !closed {
            let o = rp(rng, 5.0);
            pts.push([base[0] + o[0], base[1] + o[1], base[2] + o[2]]);
        }
        let n = pts.len();
        let segs = if closed { n } else { n - 1 };
        for i in 0..segs {
            let (Some(&p), Some(&q0)) = (pts.get(i), pts.get((i + 1) % n)) else {
                continue;
            };
            let mut q = q0;
            if rng.chance(0.2) {
                // a small gap, sometimes within the tolerance
                let j = rp(rng, tol);
                q = [q[0] + j[0], q[1] + j[1], q[2] + j[2]];
            }
            let s = if rng.chance(0.5) {
                gen_segment(rng, p, q)
            } else {
                gen_segment(rng, q, p)
            };
            curves.push(Some(resolve(&s).unwrap_or(s)));
        }
    }
    let del = rng.chance(0.5);
    if !del && rng.chance(0.2) && !curves.is_empty() {
        let i = rng.below(curves.len() as u64) as usize;
        if let Some(dup) = curves.get(i).cloned() {
            curves.push(dup);
        }
    }
    if rng.chance(0.2) {
        curves.push(None);
    }
    // shuffle
    for i in (1..curves.len()).rev() {
        let j = rng.below(i as u64 + 1) as usize;
        curves.swap(i, j);
    }
    OpIn::FixChain { curves, tol, del }
}

fn gen_vecs(rng: &mut Rng, n: usize) -> Vec<[f64; 3]> {
    (0..n)
        .map(|i| {
            if i == 1 && rng.chance(0.7) {
                // a unit normal / direction
                let v = rp(rng, 1.0);
                let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-3);
                [v[0] / l, v[1] / l, v[2] / l]
            } else {
                rp(rng, 10.0)
            }
        })
        .collect()
}

fn needed_vectors(prop: i32) -> usize {
    match prop {
        0 | 4 | 5 | 6 | 16 | 23 | 25 => 1,
        17..=22 | 24 | 26..=28 => 2,
        _ => 0,
    }
}

fn gen_property(rng: &mut Rng) -> OpIn {
    let curve = gen_curve(rng);
    let prop = rng.below(29) as i32;
    let need = needed_vectors(prop);
    let nv = if matches!(prop, 20 | 21) {
        2
    } else if rng.chance(0.08) {
        0
    } else {
        need.max(usize::from(rng.chance(0.2)))
    };
    let mut val = if rng.chance(0.9) {
        Some(rng.uniform(0.1, 20.0))
    } else {
        None
    };
    if matches!(prop, 22 | 23) && nv > 0 && val.is_none() {
        val = Some(1.0);
    }
    let t = gen_t(rng, &curve);
    OpIn::Property {
        curve,
        prop,
        val,
        vecs: gen_vecs(rng, nv),
        sqtol: *rng.choose(&[0.0, 1e-12, 1e-6, 1.0]).unwrap_or(&0.0),
        t,
        signal: rng.chance(0.5),
    }
}

fn gen_operand(rng: &mut Rng, planar: bool) -> OperandIn {
    let flat = |v: [f64; 3]| if planar { [v[0], v[1], 0.0] } else { v };
    match rng.below(5) {
        0 | 1 => {
            let mut l = gen_line(rng);
            l.p = flat(l.p);
            l.v = flat(l.v);
            if l.v.iter().all(|c| c.abs() < 1e-3) {
                l.v = [1.0, 0.5, 0.0];
            }
            OperandIn::Line(l)
        }
        2 | 3 => {
            let r = rng.uniform(0.5, 8.0);
            let circle = rng.chance(0.8);
            let d0 = rng.uniform(-90.0, 90.0);
            OperandIn::Ellipse(EllipseIn {
                c: flat(rp(rng, 5.0)),
                x: [1.0, 0.0, 0.0],
                y: if planar {
                    [0.0, 1.0, 0.0]
                } else {
                    [0.0, rng.uniform(0.5, 1.0), rng.uniform(-0.5, 0.5)]
                },
                domain: [d0, d0 + rng.uniform(30.0, 360.0)],
                radii: [r, if circle { r } else { r * rng.uniform(0.3, 0.9) }],
                inside_out: rng.chance(0.2),
            })
        }
        _ => OperandIn::Curve(gen_curve(rng)),
    }
}

fn gen_frac(rng: &mut Rng) -> [f64; 2] {
    if rng.chance(0.5) {
        [0.0, 1.0]
    } else {
        let a = rng.uniform(-0.2, 0.6);
        [a, a + rng.uniform(0.2, 0.8)]
    }
}

#[allow(clippy::too_many_lines)]
fn gen_case(rng: &mut Rng) -> OpIn {
    match rng.below(28) {
        0 | 1 => {
            let curve = if rng.chance(0.5) {
                gen_closed_curve(rng)
            } else {
                gen_curve(rng)
            };
            let mut ivl = gen_ivl(rng, &curve);
            if rng.chance(0.3) {
                ivl = {
                    let (a, b) = range_of(&curve);
                    [a, b]
                };
            }
            OpIn::IsClosed {
                base: rng.chance(0.5),
                ivl,
                tol: *rng.choose(&[0.0, 0.0, 1e-6, 0.1, 10.0]).unwrap_or(&0.0),
                curve,
            }
        }
        2 | 3 => {
            let curve = if rng.chance(0.3) {
                gen_closed_curve(rng)
            } else {
                gen_curve(rng)
            };
            let ivl = if rng.chance(0.5) {
                None
            } else {
                Some(gen_ivl(rng, &curve))
            };
            OpIn::IsDegenerate {
                base: rng.chance(0.5),
                tol: *rng.choose(&[1e-12, 0.0, 1e-6, 0.1, 100.0]).unwrap_or(&1e-12),
                ivl,
                curve,
            }
        }
        4 | 5 => {
            let curve = gen_curve(rng);
            OpIn::PolarBox {
                ivl: gen_ivl(rng, &curve),
                count: *rng.choose(&[0, 1, 2, 5, 6, 11, 40]).unwrap_or(&5),
                curve,
            }
        }
        6 => {
            let curve = gen_curve(rng);
            OpIn::Snap {
                t: gen_t(rng, &curve),
                tol: *rng.choose(&[0.0, 1e-6, 0.01, 0.2, 1e9]).unwrap_or(&0.01),
                curve,
            }
        }
        7 => OpIn::Assert {
            curve: gen_curve(rng),
            owner: rng.chance(0.5),
            given: rng.chance(0.3),
        },
        8..=10 => gen_chain(rng),
        11..=13 => {
            let c1 = gen_curve(rng);
            let c2 = gen_curve(rng);
            let ivl1 = gen_ivl(rng, &c1);
            let ivl2 = gen_ivl(rng, &c2);
            let pick = |rng: &mut Rng, ivl: [f64; 2], c: &CurveIn| match rng.below(4) {
                0 => ivl[0],
                1 => ivl[1],
                _ => gen_t(rng, c),
            };
            let s = [pick(rng, ivl1, &c1), pick(rng, ivl2, &c2)];
            let kk = |rng: &mut Rng| rng.below(14) as u8;
            OpIn::Filter {
                k1: [kk(rng), kk(rng)],
                k2: [kk(rng), kk(rng)],
                adj: rng.below(3),
                value: *rng
                    .choose(&[0.0, 1e-13, 1e-11, 1e-8, 1e-3, 0.5, 3.0])
                    .unwrap_or(&0.0),
                c1,
                ivl1,
                c2,
                ivl2,
                s,
            }
        }
        14 | 15 => {
            let curve = gen_curve(rng);
            OpIn::ProjDist {
                t: gen_t(rng, &curve),
                p: rp(rng, 10.0),
                n: gen_vecs(rng, 2).get(1).copied().unwrap_or([0.0, 0.0, 1.0]),
                rev: rng.chance(0.5),
                d: rp(rng, 1.0),
                sop: *rng.choose(&[0, 3, 4, 11, 12, 13, 14, 15]).unwrap_or(&14),
                curve,
            }
        }
        16..=20 => gen_property(rng),
        21 | 22 => {
            let c1 = gen_curve(rng);
            let c2 = gen_curve(rng);
            OpIn::CcTangent {
                x: [gen_t(rng, &c1), gen_t(rng, &c2)],
                jac: rng.chance(0.7),
                c1,
                c2,
            }
        }
        23 => {
            let curve = gen_curve(rng);
            OpIn::CpDistAt {
                p: rp(rng, 10.0),
                dsq: rng.uniform(0.0, 50.0),
                sqtol: 0.0,
                x: [gen_t(rng, &curve), rng.uniform(0.0, 10.0), rng.uniform(0.0, 10.0), rng.uniform(0.0, 10.0)],
                jac: rng.chance(0.7),
                curve,
            }
        }
        24 => {
            let c1 = gen_curve(rng);
            let c2 = gen_curve(rng);
            let (a, bb) = range_of(&c1);
            let (a2, b2) = range_of(&c2);
            let lo = a.max(a2);
            let hi = bb.min(b2);
            let t = if lo < hi {
                lo + (hi - lo) * rng.unit()
            } else {
                gen_t(rng, &c1)
            };
            OpIn::DistExtrema {
                dom: [a, bb],
                tol: *rng.choose(&[1e-12, 1e-6, 1.0, 1e6]).unwrap_or(&1e-6),
                t,
                c1,
                c2,
            }
        }
        25 | 26 => {
            let planar = rng.chance(0.7);
            OpIn::Analytic {
                a: gen_operand(rng, planar),
                fa: gen_frac(rng),
                b: gen_operand(rng, planar),
                fb: gen_frac(rng),
                tol: *rng.choose(&[1e-9, 1e-6, 1e-3]).unwrap_or(&1e-6),
            }
        }
        _ => {
            let curve = gen_curve(rng);
            let np = if rng.chance(0.6) {
                0
            } else {
                1 + rng.below(2) as usize
            };
            let planes = (0..np)
                .map(|_| {
                    let n = gen_vecs(rng, 2).get(1).copied().unwrap_or([0.0, 0.0, 1.0]);
                    [rp(rng, 10.0), n]
                })
                .collect();
            OpIn::Planes { curve, planes }
        }
    }
}

/// Whether the Rust side would delete a duplicate in `op` (upstream then uses the
/// deleted curve).
fn deletes_duplicates(op: &OpIn) -> bool {
    let OpIn::FixChain { curves, tol, del } = op else {
        return false;
    };
    if !*del {
        return false;
    }
    let mut list = Vec::new();
    for c in curves {
        match c {
            Some(c) => match build(c) {
                Ok(c) => list.push(Some(c)),
                Err(_) => return true,
            },
            None => list.push(None),
        }
    }
    fix_chain_curves_list(&Kernel::default(), list, *tol, true).map_or(true, |r| r.deleted > 0)
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.curve_p4"
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
        let k = &Kernel::default();
        rust(k, &case.parse()?)
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Generated cases cover every op with both successful and failing statuses.
    #[test]
    fn generated_cases_are_diverse() {
        let k = &Kernel::default();
        let mut rng = Rng::new(7);
        let mut stats: BTreeMap<String, [usize; 4]> = BTreeMap::new();
        let mut trues: BTreeMap<String, usize> = BTreeMap::new();
        for _ in 0..3000 {
            let op = gen_case(&mut rng);
            let name = serde_json::to_value(&op).unwrap()["op"].as_str().unwrap().to_string();
            let out = rust(k, &op);
            let e = stats.entry(name.clone()).or_default();
            e[0] += 1;
            match out {
                Err(_) => e[3] += 1,
                Ok(o) => {
                    if o.deferred.is_some() {
                        e[3] += 1;
                    } else if o.ints.get("status").is_some_and(|s| s == &vec![0]) {
                        e[1] += 1;
                        // first output: a flag, count or value
                        if o.values.get("out").and_then(|v| v.first()).is_some_and(|&x| x != 0.0) {
                            *trues.entry(name.clone()).or_default() += 1;
                        }
                    } else {
                        e[2] += 1;
                    }
                }
            }
        }
        for (k, v) in &stats {
            eprintln!("{k}: total {} ok {} err {} other {}", v[0], v[1], v[2], v[3]);
        }
        eprintln!("first output nonzero: {trues:?}");
        assert_eq!(stats.len(), 14);
    }
}
