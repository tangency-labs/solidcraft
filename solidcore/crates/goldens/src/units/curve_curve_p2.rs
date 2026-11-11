//! Runner for unit `curve.curve_p2`: the `SmCurve` base class (type queries, the
//! virtual interface over B-spline and Hermite curves, the base numerics), the
//! file-static helpers of `SmCurve.cpp` and the box tests of its global solvers.
//!
//! Ported from upstream `source/SMLib/src/SmCurve.cpp`; checked against its golden. Rust:
//! `solidcore::curve::curve::p2`.
//!
//! A case is an optional `curve` (`{"bspline": {dim, oob, cpts, p, u}}` with control points
//! as stored `[x, y, z, w]`, or `{"hermite": {dim, p1, d1, p2, d2}}`) and an `op` with its
//! parameters. The Rust side produces a flat record; the runner reports its first entry as
//! the `status` int (`0` = `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust
//! `InvalidInput`, `-1` for a Rust error upstream cannot produce) and the rest as the `out`
//! values.
//!
//! Deferred cases: B-spline paths into unported units (`HasRepeatedEndControlPoints` ->
//! `SmCurve::IsDegenerate`, `curve.curve_p4`; `CreateAnalyticCurve` of lines ->
//! `SmLine`, `curve.line`; ...) and `ReadFromDB` of the analytic and generated curve
//! types. Every run has at least [`MIN_GENERATED`] generated cases.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::{
    add_to_curve, convert_to_parameter_delta, euclidian_step_off, evaluate_continuity,
    evaluate_geometric, evaluate_step, find_test_params, flush_crv, pop_stack, proj_signed_angle,
    push_point, snap_sol_to_specified_endpoint, stack_look, test_step, unitize_tangent,
    CcGlobalSolver, CpGlobalSolver, Curve, CurveKind, CurvePropertyGs, CurvePropertyType,
    LengthIntegrand,
};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::Line;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::global_solver::{Branch, SolverOperation};
use solidcore::geom::solution_array::{ObjectKind, SolutionObject};
use solidcore::geom::tree::{AuxDataType, NodeData, NodeId, Tree, TreeNode};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_curve_n_lib::TrimMode;
use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore::SolidError;

use crate::case::CaseContext;
use crate::cases::contract::{self as contract, Limits};
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Most `ApproximateLength` samples a case runs.
const MAX_SAMPLES: usize = 400;

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `curve.curve_p2` runner.
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
#[serde(rename_all = "snake_case")]
enum CurveIn {
    Bspline(BSplineIn),
    Hermite(HermiteIn),
    Circle(CircleIn),
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
    let cur = arc
        .gw_nurb()
        .ok_or_else(|| harness("circle without NURB"))?;
    Ok(CurveIn::Bspline(BSplineIn {
        dim: 3,
        oob: false,
        cpts: cur.cpts().iter().map(|p| [p.x, p.y, p.z, p.w]).collect(),
        p: cur.degree(),
        u: cur.knots().to_vec(),
    }))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NodeIn {
    p: i64,
    c1: i64,
    c2: i64,
    aux: i64,
    data: bool,
    bbox: [f64; 6],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Info,
    Eval {
        t: f64,
        n: usize,
        left: bool,
        nonzero: bool,
    },
    EvalPoint {
        t: f64,
    },
    Ends,
    Knots {
        ivl: Option<[f64; 2]>,
    },
    Continuities {
        angle: f64,
    },
    ApproxLength {
        a: f64,
        b: f64,
        samples: usize,
        turn: bool,
    },
    EvalStep {
        t: f64,
        n: usize,
        left: bool,
    },
    EvalGeom {
        t: f64,
        n: usize,
        left: bool,
    },
    EvalContinuity {
        t: f64,
        left: bool,
        other: CurveIn,
        t2: f64,
        left2: bool,
        angle: f64,
    },
    StepOff {
        a: f64,
        b: f64,
        start: f64,
        same: bool,
        dist: f64,
        tol: f64,
    },
    Split {
        t: f64,
    },
    ConvertT {
        t: f64,
    },
    Repeated {
        tol: f64,
    },
    Equal {
        other: Option<CurveIn>,
    },
    Copy,
    Trim {
        a: f64,
        b: f64,
    },
    Transform {
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: Option<[f64; 3]>,
    },
    EditParam {
        a: f64,
        b: f64,
    },
    Reverse {
        a: f64,
        b: f64,
    },
    Convert2d,
    Analytic {
        a: f64,
        b: f64,
    },
    StepInversion {
        p: [f64; 3],
    },
    DropFast {
        p: [f64; 3],
    },
    IntersectLine,
    Snap {
        a: f64,
        b: f64,
        vec: Option<[f64; 3]>,
        sol: f64,
        tol: f64,
    },
    LengthIfeo {
        t: f64,
    },
    ReadDb {
        #[serde(rename = "type")]
        curve_type: i64,
        dim: u64,
        version: u64,
        text: String,
    },
    Flush {
        tol: f64,
        analytics: bool,
        knots: Vec<f64>,
        pts: Vec<[f64; 3]>,
    },
    AddToCurve {
        last: [[f64; 3]; 2],
        curr: [[f64; 3]; 2],
        lt: f64,
        ct: f64,
        knots: Vec<f64>,
        pts: Vec<[f64; 3]>,
    },
    Stack {
        init: Vec<f64>,
        ops: Vec<StackOp>,
    },
    ParamDelta {
        tan: [f64; 3],
        delta: [f64; 3],
    },
    UnitizeTan {
        pvv: [[f64; 3]; 3],
        gap: [f64; 3],
    },
    ProjAngle {
        p1: [f64; 3],
        p2: [f64; 3],
        c: [f64; 3],
        n: [f64; 3],
        side: [f64; 3],
    },
    TestParams {
        a: f64,
        b: f64,
        deg: usize,
        knots: Vec<f64>,
    },
    TestStep {
        cur_t: f64,
        step: f64,
        cur: [[f64; 3]; 2],
        next: [[f64; 3]; 2],
        chord: f64,
        angle: f64,
        dsq: f64,
    },
    Solver(SolverIn),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "k", rename_all = "snake_case")]
enum StackOp {
    Push { t: f64, p: [f64; 3], v: [f64; 3] },
    Pop,
    Look,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SolverIn {
    /// `property`, `cc` or `cp`.
    kind: String,
    trees: Vec<Vec<NodeIn>>,
    sop: i64,
    tol: f64,
    best: f64,
    opt: [[f64; 3]; 3],
    #[serde(default)]
    prop: i64,
    #[serde(default)]
    value: f64,
    /// 0 = `BranchMayContainAnswers`, 1 = `FindBestSubdivisionNode`.
    which: i64,
    nodes: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    curve: Option<CurveIn>,
    #[serde(flatten)]
    op: OpIn,
}

// ---- C++ input --------------------------------------------------------------------

fn b(x: bool) -> f64 {
    f64::from(u8::from(x))
}

fn to_output(flat: &[f64]) -> Output {
    let status = flat.first().copied().unwrap_or(f64::NAN);
    let rest = flat.get(1..).unwrap_or(&[]).to_vec();
    Output::new()
        .ints("status", vec![status as i64])
        .values("out", rest)
}

// ---- input contract ---------------------------------------------------------------

/// The curve block bounds of the C++ reference wrapper.
const LIMITS: Limits = Limits {
    max_index: 512,
    max_degree: 9,
    max_coord: 1e6,
};
/// How far outside its domain, in domain lengths, a curve is evaluated.
const MAX_REL_DIST: f64 = 4.0;
/// The longest knot, point or parameter stack array.
const MAX_ARRAY: usize = 1024;
/// The longest database record.
const MAX_DB_BYTES: usize = 1 << 16;
/// The most nodes of a solver tree.
const MAX_NODES: usize = 16;
/// The solver operations the cases use.
const SOLVER_OPS: [i64; 6] = [0, 2, 3, 4, 5, 22];

fn fin(v: f64) -> bool {
    contract::finite(v, LIMITS.max_coord)
}

fn vec3(v: &[f64; 3]) -> bool {
    contract::vec3(v, LIMITS.max_coord)
}

/// A curve that keeps the contract: whether it is a B-spline, and its domain.
#[derive(Debug, Clone, Copy)]
struct Domain {
    bspline: bool,
    a: f64,
    b: f64,
}

fn domain(c: &CurveIn) -> Option<Domain> {
    match resolve(c).ok()? {
        CurveIn::Bspline(bs) => {
            let s = contract::bspline(&bs.cpts, bs.p, &bs.u, &LIMITS)?;
            (2..=3).contains(&bs.dim).then_some(Domain {
                bspline: true,
                a: s.a,
                b: s.b,
            })
        }
        CurveIn::Hermite(h) => ((2..=3).contains(&h.dim)
            && contract::hermite([&h.p1, &h.d1, &h.p2, &h.d2], LIMITS.max_coord))
        .then_some(Domain {
            bspline: false,
            a: 0.0,
            b: 1.0,
        }),
        CurveIn::Circle(_) => None,
    }
}

/// Whether `t` is finite and at most [`MAX_REL_DIST`] domain lengths outside the curve's.
fn param(c: Domain, t: f64) -> bool {
    let far = MAX_REL_DIST * (c.b - c.a);
    fin(t) && t >= c.a - far && t <= c.b + far
}

fn angle(a: f64) -> bool {
    fin(a) && (0.0..=90.0).contains(&a)
}

/// Whether the op on curve `c` keeps the contract.
fn curve_op(op: &OpIn, c: Domain) -> bool {
    match op {
        OpIn::Info | OpIn::Ends | OpIn::Copy | OpIn::Convert2d | OpIn::IntersectLine => true,
        OpIn::StepInversion { p } | OpIn::DropFast { p } => vec3(p),
        OpIn::Eval { t, n, .. } | OpIn::EvalStep { t, n, .. } | OpIn::EvalGeom { t, n, .. } => {
            param(c, *t) && *n <= 3
        }
        OpIn::EvalPoint { t } | OpIn::Split { t } | OpIn::ConvertT { t } => param(c, *t),
        OpIn::LengthIfeo { t } => c.bspline && param(c, *t),
        OpIn::Knots { ivl } => {
            let [a, b] = ivl.unwrap_or([0.0, 0.0]);
            param(c, a) && param(c, b)
        }
        OpIn::Continuities { angle: a } => angle(*a),
        OpIn::ApproxLength { a, b, samples, .. } => {
            param(c, *a) && param(c, *b) && *samples <= MAX_SAMPLES
        }
        OpIn::EvalContinuity {
            t,
            other,
            t2,
            angle: a,
            ..
        } => param(c, *t) && domain(other).is_some_and(|o| param(o, *t2)) && angle(*a),
        OpIn::StepOff {
            a,
            b,
            start,
            dist,
            tol,
            ..
        } => {
            param(c, *a)
                && param(c, *b)
                && param(c, *start)
                && fin(*dist)
                && fin(*tol)
                && *tol > 0.0
        }
        OpIn::Repeated { tol } => fin(*tol) && *tol >= 0.0,
        OpIn::Equal { other } => other.as_ref().is_none_or(|o| domain(o).is_some()),
        OpIn::Trim { a, b } | OpIn::Reverse { a, b } | OpIn::Analytic { a, b } => {
            param(c, *a) && param(c, *b)
        }
        OpIn::EditParam { a, b } => {
            param(c, *a) && param(c, *b) && (b - a).abs() >= 1e-6 * (c.b - c.a)
        }
        OpIn::Transform {
            origin,
            x,
            y,
            scale,
        } => [origin, x, y, &scale.unwrap_or([1.0; 3])]
            .iter()
            .all(|v| vec3(v)),
        OpIn::Snap {
            a,
            b,
            vec,
            sol,
            tol,
        } => {
            param(c, *a)
                && param(c, *b)
                && vec3(&vec.unwrap_or([0.0; 3]))
                && param(c, *sol)
                && fin(*tol)
        }
        _ => false,
    }
}

/// The `(aux, data)` of each node of a solver tree that keeps the contract: at most
/// [`MAX_NODES`] nodes, links within the tree, children after their parent (or a leaf),
/// an aux code up to 8 and a finite box that is not inverted.
fn tree(t: &[NodeIn]) -> Option<Vec<(i64, bool)>> {
    let nn = t.len() as i64;
    if t.is_empty() || t.len() > MAX_NODES {
        return None;
    }
    t.iter()
        .enumerate()
        .map(|(k, n)| {
            let link = |v: i64| (-1..nn).contains(&v);
            let leaf = n.c1 == -1 && n.c2 == -1;
            let inner = n.c1 > k as i64 && n.c2 > k as i64;
            let [x0, y0, z0, x1, y1, z1] = n.bbox;
            (link(n.p)
                && link(n.c1)
                && link(n.c2)
                && (leaf || inner)
                && (0..=8).contains(&n.aux)
                && n.bbox.iter().all(|&v| fin(v))
                && x0 <= x1
                && y0 <= y1
                && z0 <= z1)
                .then_some((n.aux, n.data))
        })
        .collect()
}

fn is_curve_aux(aux: i64) -> bool {
    aux == 1 || aux == 2
}

/// Whether a solver case keeps the contract: one tree for the property solver and two for
/// the others, an operation the cases use, a best answer in `[0, 1e40]`, a property code up
/// to 28, `FindBestSubdivisionNode` only for the property solver, a node of each tree, and
/// no test that reads a span's data where a box is expected.
fn solver(s: &SolverIn) -> bool {
    let property = s.kind == "property";
    if !(property || s.kind == "cc" || s.kind == "cp") {
        return false;
    }
    let ntrees = if property { 1 } else { 2 };
    let Some(trees) = s.trees.iter().map(|t| tree(t)).collect::<Option<Vec<_>>>() else {
        return false;
    };
    let head = trees.len() == ntrees
        && SOLVER_OPS.contains(&s.sop)
        && fin(s.tol)
        && s.best.is_finite()
        && (0.0..=1e40).contains(&s.best)
        && s.opt.iter().all(vec3)
        && (!property || ((0..=28).contains(&s.prop) && fin(s.value)))
        && (s.which == 0 || (s.which == 1 && property))
        && s.nodes.len() == ntrees;
    let Some(nodes) = trees
        .iter()
        .zip(&s.nodes)
        .map(|(t, &i)| t.get(i).copied())
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };
    if !head {
        return false;
    }
    if s.which != 0 {
        return true;
    }
    if property {
        let (aux, data) = nodes.first().copied().unwrap_or((0, false));
        let box_props = [16, 17, 18, 19, 20, 21, 22, 23];
        // a span node's data is read only by the box properties, and not by the plane test
        !data || (!is_curve_aux(aux) || box_props.contains(&s.prop)) && !(aux == 1 && s.prop == 16)
    } else if s.kind == "cc" {
        let span = nodes.iter().all(|&(aux, data)| data && is_curve_aux(aux));
        !((s.sop == 5 || s.sop == 22) && span)
    } else {
        let (aux, data) = nodes.get(1).copied().unwrap_or((0, false));
        !(s.sop == 5 && data && is_curve_aux(aux))
    }
}

fn knots_and_points(knots: &[f64], pts: &[[f64; 3]]) -> bool {
    knots.len() <= MAX_ARRAY
        && knots.iter().all(|&k| fin(k))
        && pts.len() <= MAX_ARRAY
        && pts.iter().all(vec3)
}

/// Whether a static op (one without a curve) keeps the contract.
fn static_op(op: &OpIn) -> Option<bool> {
    Some(match op {
        OpIn::ReadDb {
            curve_type,
            dim,
            version,
            text,
        } => {
            (0..=1 << 20).contains(curve_type)
                && (2..=3).contains(dim)
                && *version <= 64
                && text.len() <= MAX_DB_BYTES
        }
        OpIn::Flush {
            tol, knots, pts, ..
        } => {
            let nk = knots.len();
            fin(*tol)
                && knots_and_points(knots, pts)
                && (nk == 0 || pts.len() == 3 * (nk - 1) + 1)
                && nk != 1
        }
        OpIn::AddToCurve {
            last,
            curr,
            lt,
            ct,
            knots,
            pts,
        } => {
            last.iter().chain(curr).all(vec3)
                && fin(*lt)
                && fin(*ct)
                && knots_and_points(knots, pts)
                && (knots.is_empty() || !pts.is_empty())
        }
        OpIn::Stack { init, ops } => {
            init.len() <= MAX_ARRAY
                && init.iter().all(|&v| fin(v))
                && ops.len() <= 64
                && ops.iter().all(|o| match o {
                    StackOp::Push { t, p, v } => fin(*t) && vec3(p) && vec3(v),
                    StackOp::Pop | StackOp::Look => true,
                })
        }
        OpIn::ParamDelta { tan, delta } => vec3(tan) && vec3(delta),
        OpIn::UnitizeTan { pvv, gap } => pvv.iter().all(vec3) && vec3(gap),
        OpIn::ProjAngle { p1, p2, c, n, side } => [p1, p2, c, n, side].iter().all(|v| vec3(v)),
        OpIn::TestParams { a, b, deg, knots } => {
            fin(*a)
                && fin(*b)
                && *deg <= 32
                && knots.len() <= MAX_ARRAY
                && knots.iter().all(|&k| fin(k))
        }
        OpIn::TestStep {
            cur_t,
            step,
            cur,
            next,
            chord,
            angle: a,
            dsq,
        } => {
            [cur_t, step, chord, a, dsq].iter().all(|&&v| fin(v))
                && cur.iter().chain(next).all(vec3)
        }
        OpIn::Solver(s) => solver(s),
        _ => return None,
    })
}

/// Whether a case keeps the input contract ported from the reference's wrapper: a curve for
/// every curve op as [`domain`] takes it (arcs within the arc constructor's `[-360, 360]`
/// degrees), parameters within [`MAX_REL_DIST`] domain lengths, derivative orders up to 3,
/// angles in `[0, 90]`, the tolerance signs, a non-degenerate `EditParam`, `LengthIfeo` of
/// a B-spline, finite values within range, and the static ops' array, record and solver
/// bounds.
fn keeps_contract(i: &Inputs) -> bool {
    if let Some(ok) = static_op(&i.op) {
        return ok;
    }
    i.curve
        .as_ref()
        .and_then(domain)
        .is_some_and(|c| curve_op(&i.op, c))
}

// ---- Rust side --------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

fn build(c: &CurveIn) -> Result<Curve, HarnessError> {
    match &resolve(c)? {
        CurveIn::Bspline(b) => {
            let n = b
                .cpts
                .len()
                .checked_sub(1)
                .ok_or_else(|| harness("no points"))?;
            let m =
                b.u.len()
                    .checked_sub(1)
                    .ok_or_else(|| harness("no knots"))?;
            let mut cur =
                allocate_nurb_curve(n, b.p, m).map_err(|e| harness(format!("curve: {e}")))?;
            for (d, s) in cur.cpts_mut().iter_mut().zip(&b.cpts) {
                d.x = s[0];
                d.y = s[1];
                d.z = s[2];
                d.w = s[3];
            }
            cur.knots_mut().copy_from_slice(&b.u);
            let mut c = BSplineCurve::from_nurb(cur, b.dim, false);
            c.set_out_of_bounds_enabled(b.oob);
            Ok(Curve::from(c))
        }
        CurveIn::Hermite(h) => Ok(Curve::from(HermiteCurve::new(
            v3(h.p1),
            v3(h.d1),
            v3(h.p2),
            v3(h.d2),
            h.dim,
        ))),
        CurveIn::Circle(_) => Err(harness("unresolved circle")),
    }
}

fn pv(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

/// The B-spline state (`0 dim n p m cpts knots`, `-1` without NURB).
fn bspline_state(o: &mut Vec<f64>, b: &BSplineCurve) {
    o.extend([0.0, b.dim() as f64]);
    match b.gw_nurb() {
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

fn state(o: &mut Vec<f64>, c: &Curve) -> Result<(), HarnessError> {
    match c.kind() {
        CurveKind::BSpline(b) => bspline_state(o, b),
        // an SmBSplineCurve (the shim's IsKindOf(SmBSplineCurve_TYPE))
        CurveKind::Parabola(p) => bspline_state(o, p.base()),
        CurveKind::Hyperbola(h) => {
            // not produced by this unit's cases (see curve.hyperbola's runner)
            o.extend([2.0, h.base().dim() as f64]);
        }
        CurveKind::Ellipse(e) => {
            // not produced by this unit's cases (see curve.ellipse_p1's runner)
            o.extend([2.0, e.base().dim() as f64]);
        }
        CurveKind::Composite(x) => {
            // not produced by this unit's cases (see curve.composite_curve_p1's runner)
            o.extend([2.0, x.dim() as f64]);
        }
        CurveKind::Hermite(h) => {
            o.extend([1.0, h.dim() as f64]);
            let a = h
                .evaluate(0.0, 1, false)
                .map_err(|e| harness(e.to_string()))?;
            let e = h
                .evaluate(1.0, 1, false)
                .map_err(|e| harness(e.to_string()))?;
            for v in a.iter().take(2).chain(e.iter().take(2)) {
                pv(o, *v);
            }
        }
        CurveKind::Iso(c) => {
            o.extend([3.0, c.dim() as f64, c.iso_parameter()]);
        }
        CurveKind::Conic(k) => {
            // the defining data evaluated back: position and tangent at 0 and 0.5
            o.extend([2.0, k.dim() as f64]);
            for t in [0.0, 0.5] {
                match k.evaluate(t, 1, false) {
                    Ok(v) => {
                        for p in v.iter().take(2) {
                            pv(o, *p);
                        }
                    }
                    Err(_) => o.push(-1.0),
                }
            }
        }
        // the shim reports another type as -1
        CurveKind::Projected(_)
        | CurveKind::Offset(_)
        | CurveKind::CrvInVolume(_)
        | CurveKind::CrvOnSurf(_)
        | CurveKind::TangentField(_)
        | CurveKind::Line(_) => o.push(-1.0),
    }
    Ok(())
}

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
        e if solidcore::error::nurbs::upstream_status(e).is_some() => {
            f64::from(solidcore::error::nurbs::upstream_status(e).unwrap_or(-1))
        }
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

fn deferred_or_status(e: SolidError) -> Result<f64, Box<Output>> {
    match Fail::from(e) {
        Fail::Deferred(d) => Err(d),
        Fail::Status(s) => Ok(s),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Obj(i64);

impl SolutionObject for Obj {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Other
    }
}

fn build_tree(nodes: &[NodeIn]) -> Result<Tree<Obj>, HarnessError> {
    let link = |v: i64| -> Option<NodeId> { usize::try_from(v).ok().map(NodeId) };
    let mut t = Tree::new(None);
    for n in nodes {
        let mut node = TreeNode::new();
        node.parent = link(n.p);
        node.child1 = link(n.c1);
        node.child2 = link(n.c2);
        node.aux_type = AuxDataType::from_code(n.aux as i32).ok_or_else(|| harness("aux type"))?;
        // stand-in for a non-null m_pData (the tests never read it)
        node.data = if n.data {
            NodeData::ObjectList(None)
        } else {
            NodeData::None
        };
        let x = n.bbox;
        node.bbox = Extent3d::from_bounds(x[0], x[1], x[2], x[3], x[4], x[5]);
        t.add_node(node);
    }
    t.set_top_node(Some(NodeId(0)));
    Ok(t)
}

fn rust_solver(k: &Kernel, s: &SolverIn) -> Result<Output, HarnessError> {
    let trees: Vec<Tree<Obj>> = s
        .trees
        .iter()
        .map(|t| build_tree(t))
        .collect::<Result<_, _>>()?;
    let sop = SolverOperation::from_code(s.sop as i32).ok_or_else(|| harness("solver op"))?;
    let opt: Vec<Vec3> = s.opt.iter().map(|&o| v3(o)).collect();
    let ivl = Extent1d::from_min_max(0.0, 1.0);
    let mut branch: Branch = [None, None, None];
    for (slot, &n) in branch.iter_mut().zip(&s.nodes) {
        *slot = Some(NodeId(n));
    }
    let mut o = vec![0.0];
    let t0 = trees.first().ok_or_else(|| harness("tree"))?;
    let r = match s.kind.as_str() {
        "property" => {
            let prop =
                CurvePropertyType::from_code(s.prop as i32).ok_or_else(|| harness("prop"))?;
            let mut g = CurvePropertyGs::new(k, t0, ivl, prop, Some(s.value), opt);
            g.base.operation = sop;
            g.base.tolerance_3d = s.tol;
            g.base.best_answer_so_far_sq = s.best;
            if s.which == 0 {
                g.may_contain_answers(&branch).map(|f| vec![b(f)])
            } else {
                g.best_subdivision_node(&branch)
                    .map(|(i, sw)| vec![i as f64, b(sw)])
            }
        }
        "cc" | "cp" => {
            let t1 = trees.get(1).ok_or_else(|| harness("tree 2"))?;
            if s.kind == "cc" {
                let mut g = CcGlobalSolver::new(t0, t1, ivl, ivl);
                g.base.operation = sop;
                g.base.tolerance_3d = s.tol;
                g.base.best_answer_so_far_sq = s.best;
                g.base.opt_vectors = opt;
                g.may_contain_answers(&branch).map(|f| vec![b(f)])
            } else {
                let mut g = CpGlobalSolver::new(k, t0, t1, ivl);
                g.base.operation = sop;
                g.base.tolerance_3d = s.tol;
                g.base.best_answer_so_far_sq = s.best;
                g.base.opt_vectors = opt;
                g.may_contain_answers(&branch).map(|f| vec![b(f)])
            }
        }
        _ => return Err(harness("solver kind")),
    };
    match r {
        Ok(v) => o.extend(v),
        Err(e) => match deferred_or_status(e) {
            Err(d) => return Ok(*d),
            Ok(st) => {
                if let Some(x) = o.first_mut() {
                    *x = st;
                }
            }
        },
    }
    Ok(to_output(&o))
}

#[allow(clippy::too_many_lines)]
fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut o: Vec<f64> = Vec::new();
    if let OpIn::Solver(s) = &i.op {
        return rust_solver(&k, s);
    }
    let curve = match &i.curve {
        Some(c) => Some(build(c)?),
        None => None,
    };
    let c_ref = || curve.as_ref().ok_or_else(|| harness("op needs a curve"));
    match &i.op {
        OpIn::Info => {
            let c = c_ref()?;
            let ivl = c.natural_interval();
            let deg = tri!(o, c.degree());
            let nnk = tri!(o, c.number_natural_knots(&k));
            let ncp = tri!(o, c.number_control_points());
            let rat = tri!(o, c.is_rational());
            o.extend([0.0, c.type_code() as f64, c.dim() as f64]);
            o.extend([deg as f64, nnk as f64, ncp as f64, ivl.min(), ivl.max()]);
            o.extend([
                b(c.is_analytic()),
                b(c.is_bounded()),
                b(rat),
                b(c.owner().is_none()),
            ]);
            for kind in [14001, 12060, 14033, 14034, 14010, 14022, 16016, 12000, 1] {
                o.push(b(c.is_kind_of(kind)));
            }
        }
        OpIn::Eval {
            t,
            n,
            left,
            nonzero,
        } => {
            let v = tri!(o, c_ref()?.evaluate(&k, *t, *n, *left, *nonzero));
            o.push(0.0);
            for x in v.iter().take(n + 1) {
                pv(&mut o, *x);
            }
        }
        OpIn::EvalPoint { t } => {
            let p = tri!(o, c_ref()?.evaluate_point(&k, *t));
            o.push(0.0);
            pv(&mut o, p);
        }
        OpIn::Ends => {
            let e = tri!(o, c_ref()?.ends(&k,));
            o.push(0.0);
            pv(&mut o, e.start);
            pv(&mut o, e.end);
            o.extend([e.start_weight, e.end_weight]);
        }
        OpIn::Knots { ivl } => {
            let iv = ivl.map(|[a, bb]| Extent1d::from_min_max(a, bb));
            let (kn, m) = tri!(o, c_ref()?.knots(&k, iv.as_ref()));
            o.extend([0.0, kn.len() as f64]);
            o.extend_from_slice(&kn);
            o.push(m.len() as f64);
            o.extend(m.iter().map(|&x| x as f64));
        }
        OpIn::Continuities { angle } => {
            let (mn, all) = tri!(o, c_ref()?.calculate_continuities(&k, *angle));
            o.extend([0.0, f64::from(mn as i32), all.len() as f64]);
            o.extend(all.iter().map(|&x| f64::from(x as i32)));
        }
        OpIn::ApproxLength {
            a,
            b: bb,
            samples,
            turn,
        } => {
            // more than `MAX_SAMPLES` samples is not a case: upstream (and the port)
            // would loop that many times
            if *samples > MAX_SAMPLES {
                return Err(HarnessError::new(format!("{samples} samples")));
            }
            let (len, t) = tri!(
                o,
                c_ref()?.approximate_length(&k, &Extent1d::from_min_max(*a, *bb), *samples, *turn)
            );
            o.extend([0.0, len, b(t.is_some())]);
            if let Some(t) = t {
                o.push(t);
            }
        }
        OpIn::EvalStep { t, n, left } => {
            let v = tri!(o, evaluate_step(&k, c_ref()?, *t, *n, *left));
            o.push(0.0);
            for x in v.iter().take(n + 1) {
                pv(&mut o, *x);
            }
        }
        OpIn::EvalGeom { t, n, left } => {
            let v = tri!(o, evaluate_geometric(&k, c_ref()?, *t, *n, *left));
            o.push(0.0);
            for x in v.iter().take(n + 1) {
                pv(&mut o, *x);
            }
        }
        OpIn::EvalContinuity {
            t,
            left,
            other,
            t2,
            left2,
            angle,
        } => {
            let other = build(other)?;
            let r = tri!(
                o,
                evaluate_continuity(&k, c_ref()?, *t, *left, &other, *t2, *left2, *angle)
            );
            o.extend([0.0, f64::from(r.continuity as i32)]);
            for v in r.this_eval.iter().chain(r.other_eval.iter()) {
                pv(&mut o, *v);
            }
        }
        OpIn::StepOff {
            a,
            b: bb,
            start,
            same,
            dist,
            tol,
        } => {
            let s = tri!(
                o,
                euclidian_step_off(
                    &k,
                    c_ref()?,
                    &Extent1d::from_min_max(*a, *bb),
                    *start,
                    *same,
                    *dist,
                    *tol
                )
            );
            o.extend([
                if s.converged { 0.0 } else { 1001.0 },
                b(s.clamped),
                s.end_param,
            ]);
        }
        OpIn::Split { t } => {
            let mut c = c_ref()?.clone();
            let nc = tri!(o, c.create_by_split_curve(&k, *t));
            o.push(0.0);
            state(&mut o, &c)?;
            state(&mut o, &nc)?;
        }
        OpIn::ConvertT { t } => {
            let c = c_ref()?;
            let a = tri!(o, c.convert_t_from_nurbs_to_step(&Kernel::default(), *t));
            let bb = tri!(o, c.convert_t_from_step_to_nurbs(&k, *t));
            o.extend([0.0, a, bb]);
        }
        OpIn::Repeated { tol } => {
            let f = tri!(o, c_ref()?.has_repeated_end_control_points(&k, *tol));
            o.extend([0.0, b(f)]);
        }
        OpIn::Equal { other } => {
            let c = c_ref()?;
            let eq = match other {
                None => tri!(o, c.approx_eq(&k, c)),
                Some(x) => tri!(o, c.approx_eq(&k, &build(x)?)),
            };
            o.extend([0.0, b(eq)]);
        }
        OpIn::Copy => {
            let nc = tri!(o, c_ref()?.copy(&Kernel::default()));
            o.push(0.0);
            state(&mut o, &nc)?;
        }
        OpIn::Trim { a, b: bb } => {
            let mut c = c_ref()?.clone();
            let mut ivl = Extent1d::from_min_max(*a, *bb);
            let st = match c.trim(&k, &mut ivl, TrimMode::Notify) {
                Ok(()) => 0.0,
                Err(e) => match deferred_or_status(e) {
                    Err(d) => return Ok(*d),
                    Ok(s) => s,
                },
            };
            o.extend([st, ivl.min(), ivl.max()]);
            state(&mut o, &c)?;
        }
        OpIn::Transform {
            origin,
            x,
            y,
            scale,
        } => {
            let mut c = c_ref()?.clone();
            let ax = Axis2Placement::new(v3(*origin), v3(*x), v3(*y));
            let st = match c.transform(&Kernel::default(), &ax, scale.map(v3)) {
                Ok(()) => 0.0,
                Err(e) => match deferred_or_status(e) {
                    Err(d) => return Ok(*d),
                    Ok(s) => s,
                },
            };
            o.push(st);
            state(&mut o, &c)?;
        }
        OpIn::EditParam { a, b: bb } => {
            let mut c = c_ref()?.clone();
            let st = match c
                .edit_parameterization(&Kernel::default(), &Extent1d::from_min_max(*a, *bb))
            {
                Ok(()) => 0.0,
                Err(e) => match deferred_or_status(e) {
                    Err(d) => return Ok(*d),
                    Ok(s) => s,
                },
            };
            o.push(st);
            state(&mut o, &c)?;
        }
        OpIn::Reverse { a, b: bb } => {
            let mut c = c_ref()?.clone();
            match c.reverse_parameterization(&Kernel::default(), &Extent1d::from_min_max(*a, *bb)) {
                Ok(nv) => o.extend([0.0, nv.min(), nv.max()]),
                Err(e) => match deferred_or_status(e) {
                    Err(d) => return Ok(*d),
                    Ok(s) => o.push(s),
                },
            }
            state(&mut o, &c)?;
        }
        OpIn::Convert2d => {
            let mut c = c_ref()?.clone();
            let st = match c.convert_to_2d(&Kernel::default()) {
                Ok(()) => 0.0,
                Err(e) => match deferred_or_status(e) {
                    Err(d) => return Ok(*d),
                    Ok(s) => s,
                },
            };
            o.push(st);
            state(&mut o, &c)?;
        }
        OpIn::Analytic { a, b: bb } => {
            let r = tri!(
                o,
                c_ref()?
                    .create_analytic_curve(&Kernel::default(), &Extent1d::from_min_max(*a, *bb))
            );
            o.extend([0.0, b(r.is_some())]);
        }
        OpIn::StepInversion { p } => {
            tri!(o, c_ref()?.step_inversion(&Kernel::default(), v3(*p)));
            o.push(0.0);
        }
        OpIn::DropFast { p } => {
            let c = c_ref()?;
            tri!(
                o,
                c.drop_point_fast(
                    &k,
                    &c.natural_interval(),
                    SolverOperation::Minimize,
                    v3(*p),
                    None,
                    1e-6,
                    None,
                    solidcore::geom::global_solver::SolutionRequested::Single,
                )
            );
            o.push(0.0);
        }
        OpIn::IntersectLine => {
            let c = c_ref()?;
            let ivl = c.natural_interval();
            let r = tri!(
                o,
                c.intersect_with_line(&Kernel::default(), &ivl, &Line::default(), &ivl, 1e-6)
            );
            o.extend([0.0, b(r.needs_more_intersections), r.solutions.len() as f64]);
        }
        OpIn::Snap {
            a,
            b: bb,
            vec,
            sol,
            tol,
        } => {
            let (s, good) = tri!(
                o,
                snap_sol_to_specified_endpoint(
                    &k,
                    c_ref()?,
                    &Extent1d::from_min_max(*a, *bb),
                    vec.map(v3),
                    *sol,
                    *tol
                )
            );
            o.extend([0.0, s, b(good)]);
        }
        OpIn::LengthIfeo { t } => {
            let CurveKind::BSpline(bs) = c_ref()?.kind() else {
                return Err(harness("LengthIfeo needs a B-spline"));
            };
            let v = tri!(o, LengthIntegrand::new(bs).evaluate(*t));
            o.extend([0.0, v]);
        }
        OpIn::ReadDb {
            curve_type,
            dim,
            version,
            text,
        } => {
            let mut db = DatabaseReader::new();
            db.open_for_read(text.clone().into_bytes(), FileType::Ascii);
            match Curve::read_from_db(&Kernel::default(), *curve_type, &mut db, *dim, *version) {
                Ok(c) => {
                    o.extend([0.0, 1.0]);
                    state(&mut o, &c)?;
                }
                Err(e) => match deferred_or_status(e) {
                    Err(d) => return Ok(*d),
                    Ok(s) => o.extend([s, 0.0]),
                },
            }
        }
        OpIn::Flush {
            tol,
            analytics,
            knots,
            pts,
        } => {
            let mut kn = knots.clone();
            let mut poly: Vec<Vec3> = pts.iter().map(|&p| v3(p)).collect();
            match flush_crv(*tol, &mut poly, &mut kn, *analytics, false) {
                Ok(r) => {
                    o.extend([0.0, b(r.is_some())]);
                    if let Some(c) = r {
                        state(&mut o, &Curve::from(c))?;
                    }
                }
                Err(e) => match deferred_or_status(e) {
                    Err(d) => return Ok(*d),
                    Ok(s) => o.extend([s, 0.0]),
                },
            }
            o.push(kn.len() as f64);
            o.extend_from_slice(&kn);
            o.push(poly.len() as f64);
            for p in &poly {
                pv(&mut o, *p);
            }
        }
        OpIn::AddToCurve {
            last,
            curr,
            lt,
            ct,
            knots,
            pts,
        } => {
            let mut kn = knots.clone();
            let mut poly: Vec<Vec3> = pts.iter().map(|&p| v3(p)).collect();
            let st = match add_to_curve(
                [v3(last[0]), v3(last[1])],
                [v3(curr[0]), v3(curr[1])],
                *lt,
                *ct,
                &mut poly,
                &mut kn,
            ) {
                Ok(()) => 0.0,
                Err(e) => match deferred_or_status(e) {
                    Err(d) => return Ok(*d),
                    Ok(s) => s,
                },
            };
            o.extend([st, kn.len() as f64]);
            o.extend_from_slice(&kn);
            o.push(poly.len() as f64);
            for p in &poly {
                pv(&mut o, *p);
            }
        }
        OpIn::Stack { init, ops } => {
            let mut stack = init.clone();
            o.push(0.0);
            for op in ops {
                match op {
                    StackOp::Push { t, p, v } => {
                        push_point(*t, [v3(*p), v3(*v)], &mut stack);
                        o.push(0.0);
                    }
                    StackOp::Pop | StackOp::Look => {
                        let r = if matches!(op, StackOp::Pop) {
                            pop_stack(&mut stack)
                        } else {
                            stack_look(&stack)
                        };
                        match r {
                            Ok((t, pvv)) => {
                                o.extend([0.0, t]);
                                pv(&mut o, pvv[0]);
                                pv(&mut o, pvv[1]);
                            }
                            Err(e) => o.push(status(&e)),
                        }
                    }
                }
                o.push(stack.len() as f64);
            }
        }
        OpIn::ParamDelta { tan, delta } => {
            o.extend([0.0, convert_to_parameter_delta(v3(*tan), v3(*delta))]);
        }
        OpIn::UnitizeTan { pvv, gap } => {
            let (u, len) = unitize_tangent([v3(pvv[0]), v3(pvv[1]), v3(pvv[2])], v3(*gap));
            o.push(0.0);
            pv(&mut o, u);
            o.push(len);
        }
        OpIn::ProjAngle { p1, p2, c, n, side } => {
            let a = tri!(
                o,
                proj_signed_angle(v3(*p1), v3(*p2), v3(*c), v3(*n), v3(*side))
            );
            o.extend([0.0, a]);
        }
        OpIn::TestParams {
            a, b: bb, knots, ..
        } => {
            let p = find_test_params(*a, *bb, knots);
            o.extend([0.0, p.len() as f64]);
            o.extend_from_slice(&p);
        }
        OpIn::TestStep {
            step,
            cur,
            next,
            chord,
            angle,
            dsq,
            ..
        } => {
            let r = test_step(
                *step,
                [v3(cur[0]), v3(cur[1])],
                [v3(next[0]), v3(next[1])],
                *chord,
                *angle,
                *dsq,
            );
            o.extend([0.0, f64::from(r as i32)]);
        }
        OpIn::Solver(_) => return Err(harness("unreachable")),
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
    let two_d = rng.chance(0.2);
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
    if inner >= 2 && rng.chance(0.3) {
        // a repeated interior knot (multiplicity 2 <= p when p >= 2)
        if p >= 2 {
            if let Some(&v) = ks.first() {
                if let Some(x) = ks.get_mut(1) {
                    *x = v;
                }
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
    let dim = if two_d || rng.chance(0.15) { 2 } else { 3 };
    BSplineIn {
        dim,
        oob: rng.chance(0.15),
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
    let mut p2 = rp(rng, s);
    let p1 = rp(rng, s);
    if rng.chance(0.05) {
        p2 = p1;
    }
    let mut h = HermiteIn {
        dim: if rng.chance(0.2) { 2 } else { 3 },
        p1,
        d1,
        p2,
        d2: rp(rng, s),
    };
    if h.dim == 2 {
        for v in [&mut h.p1, &mut h.d1, &mut h.p2, &mut h.d2] {
            v[2] = 0.0;
        }
    }
    h
}

fn gen_curve(rng: &mut Rng) -> CurveIn {
    if rng.chance(0.08) {
        let a0 = rng.uniform(-90.0, 90.0);
        let circle = CurveIn::Circle(CircleIn {
            center: rp(rng, 5.0),
            x: [1.0, 0.0, 0.0],
            y: [0.0, rng.uniform(0.5, 1.0), rng.uniform(-0.5, 0.5)],
            radius: rng.uniform(0.5, 10.0),
            a0,
            a1: a0 + rng.uniform(20.0, 360.0),
        });
        if let Ok(c) = resolve(&circle) {
            return c;
        }
    }
    if rng.chance(0.65) {
        CurveIn::Bspline(gen_bspline(rng))
    } else {
        CurveIn::Hermite(gen_hermite(rng))
    }
}

fn range_of(c: &CurveIn) -> (f64, f64) {
    let resolved = resolve(c).ok();
    match resolved.as_ref().unwrap_or(c) {
        CurveIn::Circle(ci) => (ci.a0, ci.a1),
        CurveIn::Bspline(b) => (
            b.u.first().copied().unwrap_or(0.0),
            b.u.last().copied().unwrap_or(1.0),
        ),
        CurveIn::Hermite(_) => (0.0, 1.0),
    }
}

/// A parameter of `c`: interior, at a knot, at an end, or slightly outside.
fn gen_t(rng: &mut Rng, c: &CurveIn) -> f64 {
    let (a, bb) = range_of(c);
    match rng.below(10) {
        0 => a,
        1 => bb,
        2 => {
            if let CurveIn::Bspline(b) = c {
                *rng.choose(&b.u).unwrap_or(&a)
            } else {
                0.5
            }
        }
        3 => a + (bb - a) * rng.uniform(-0.3, 1.3),
        _ => a + (bb - a) * rng.unit(),
    }
}

fn gen_ivl(rng: &mut Rng, c: &CurveIn) -> (f64, f64) {
    let (a, bb) = range_of(c);
    if rng.chance(0.3) {
        return (a, bb);
    }
    let x = a + (bb - a) * rng.uniform(-0.2, 1.2);
    let y = a + (bb - a) * rng.uniform(-0.2, 1.2);
    (x.min(y), x.max(y))
}

fn hermite_text(h: &HermiteIn) -> String {
    let c = HermiteCurve::new(v3(h.p1), v3(h.d1), v3(h.p2), v3(h.d2), 3);
    let (pc, pd) = c.power_basis();
    let mut s = String::new();
    for v in [v3(h.p1), v3(h.d1), v3(h.p2), v3(h.d2), pc, pd] {
        let _ = writeln!(s, "{:.17e} {:.17e} {:.17e}", v.x, v.y, v.z);
    }
    s
}

fn bspline_text(b: &BSplineIn) -> String {
    let n = b.cpts.len() - 1;
    let rat = b.cpts.first().is_some_and(|c| c[3] != NOW);
    let two_d = b.cpts.first().is_some_and(|c| c[2] == NOZ);
    let mut s = format!(
        "{}\n{}\n{}\n{}\n1\n6\n",
        n,
        b.p,
        i32::from(rat),
        if two_d { 2 } else { 3 }
    );
    for c in &b.cpts {
        let mut vals = vec![c[0], c[1]];
        if !two_d {
            vals.push(c[2]);
        }
        if rat {
            vals.push(c[3]);
        }
        let t: Vec<String> = vals.iter().map(|x| format!("{x:.17e}")).collect();
        s.push_str(&t.join(" "));
        s.push('\n');
    }
    for k in &b.u {
        let _ = writeln!(s, "{k:.17e}");
    }
    s
}

fn gen_tree(rng: &mut Rng, curvey: bool) -> Vec<NodeIn> {
    let mut nodes = vec![];
    let lo = rp(rng, 10.0);
    let size = [
        rng.uniform(0.0, 8.0),
        rng.uniform(0.0, 8.0),
        rng.uniform(0.0, 8.0),
    ];
    let bbox = |lo: [f64; 3], s: [f64; 3]| {
        [
            lo[0],
            lo[1],
            lo[2],
            lo[0] + s[0],
            lo[1] + s[1],
            lo[2] + s[2],
        ]
    };
    let aux = |rng: &mut Rng| -> i64 {
        if curvey && rng.chance(0.6) {
            1 + rng.below(2) as i64
        } else {
            *rng.choose(&[0, 6, 8]).unwrap_or(&0)
        }
    };
    let inner = rng.chance(0.7);
    let a0 = aux(rng);
    let d0 = rng.chance(0.8);
    nodes.push(NodeIn {
        p: -1,
        c1: if inner { 1 } else { -1 },
        c2: if inner { 2 } else { -1 },
        aux: a0,
        data: d0,
        bbox: bbox(lo, size),
    });
    if inner {
        for _ in 0..2 {
            let l = [
                lo[0] + rng.unit() * size[0] * 0.5,
                lo[1] + rng.unit() * size[1] * 0.5,
                lo[2] + rng.unit() * size[2] * 0.5,
            ];
            let s = [size[0] * 0.5, size[1] * 0.5, size[2] * 0.5];
            let a = aux(rng);
            let d = rng.chance(0.8);
            nodes.push(NodeIn {
                p: 0,
                c1: -1,
                c2: -1,
                aux: a,
                data: d,
                bbox: bbox(l, s),
            });
        }
    }
    nodes
}

fn gen_solver(rng: &mut Rng) -> SolverIn {
    let kind = *rng
        .choose(&["property", "property", "property", "cc", "cp"])
        .unwrap_or(&"property");
    let ntrees = if kind == "property" { 1 } else { 2 };
    let trees: Vec<Vec<NodeIn>> = (0..ntrees).map(|_| gen_tree(rng, true)).collect();
    let nodes = trees
        .iter()
        .map(|t| rng.below(t.len() as u64) as usize)
        .collect();
    let mut opt = [rp(rng, 10.0), rp(rng, 1.0), rp(rng, 1.0)];
    if rng.chance(0.5) {
        // unit plane normal / reference vector
        for v in opt.iter_mut().skip(1) {
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
            *v = [v[0] / l, v[1] / l, v[2] / l];
        }
    }
    SolverIn {
        kind: kind.to_string(),
        trees,
        sop: *rng.choose(&[0, 2, 3, 4, 5, 5, 22, 22]).unwrap_or(&5),
        tol: *rng.choose(&[0.0, 1e-6, 0.01, 0.5]).unwrap_or(&0.0),
        best: *rng.choose(&[1e20, 0.0, 1.0, 25.0, 0.1]).unwrap_or(&1e20),
        opt,
        prop: rng.below(29) as i64,
        value: rng.uniform(-5.0, 15.0),
        which: i64::from(kind == "property" && rng.chance(0.4)),
        nodes,
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let c = gen_curve(rng);
    let curve = Some(c.clone());
    let bs = |rng: &mut Rng| -> f64 { rng.uniform(-1.0, 1.0) };
    let op = match rng.below(40) {
        0 => OpIn::Info,
        1 | 2 => OpIn::Eval {
            t: gen_t(rng, &c),
            n: rng.below(4) as usize,
            left: rng.chance(0.5),
            nonzero: rng.chance(0.5),
        },
        3 => OpIn::EvalPoint { t: gen_t(rng, &c) },
        4 => OpIn::Ends,
        5 => OpIn::Knots {
            ivl: rng.chance(0.5).then(|| {
                let (a, b) = gen_ivl(rng, &c);
                [a, b]
            }),
        },
        6 => OpIn::Continuities {
            angle: *rng.choose(&[0.5, 1.0, 5.0, 30.0]).unwrap_or(&0.5),
        },
        7 | 8 => {
            let (a, b) = gen_ivl(rng, &c);
            OpIn::ApproxLength {
                a,
                b,
                samples: rng.below(60) as usize,
                turn: rng.chance(0.6),
            }
        }
        9 => OpIn::EvalStep {
            t: gen_t(rng, &c),
            n: rng.below(4) as usize,
            left: rng.chance(0.5),
        },
        10 | 11 => OpIn::EvalGeom {
            t: gen_t(rng, &c),
            n: rng.below(4) as usize,
            left: rng.chance(0.5),
        },
        12..=14 => {
            let same = rng.chance(0.4);
            let other = if same { c.clone() } else { gen_curve(rng) };
            let t = gen_t(rng, &c);
            OpIn::EvalContinuity {
                t,
                left: rng.chance(0.5),
                t2: if same { t } else { gen_t(rng, &other) },
                other,
                left2: rng.chance(0.5),
                angle: *rng.choose(&[0.5, 1.0, 10.0]).unwrap_or(&0.5),
            }
        }
        15 | 16 => {
            let (a, b) = range_of(&c);
            OpIn::StepOff {
                a,
                b,
                start: gen_t(rng, &c).clamp(a, b),
                same: rng.chance(0.5),
                dist: *rng.choose(&[0.0, 0.1, 1.0, 5.0, 50.0]).unwrap_or(&1.0)
                    * rng.uniform(0.5, 1.5),
                tol: *rng.choose(&[1e-6, 1e-9, 1e-3]).unwrap_or(&1e-6),
            }
        }
        17 | 18 => OpIn::Split { t: gen_t(rng, &c) },
        19 => OpIn::ConvertT { t: gen_t(rng, &c) },
        20 => OpIn::Repeated {
            tol: *rng.choose(&[1e-6, 0.01, 1.0]).unwrap_or(&1e-6),
        },
        21 => OpIn::Equal {
            other: match rng.below(3) {
                0 => None,
                1 => Some(c.clone()),
                _ => Some(gen_curve(rng)),
            },
        },
        22 => OpIn::Copy,
        23 | 24 => {
            let (a, b) = gen_ivl(rng, &c);
            OpIn::Trim { a, b }
        }
        25 => OpIn::Transform {
            origin: rp(rng, 5.0),
            x: rp(rng, 1.0),
            y: rp(rng, 1.0),
            scale: rng.chance(0.5).then(|| {
                if rng.chance(0.5) {
                    let s = rng.uniform(0.2, 3.0);
                    [s, s, s]
                } else {
                    [
                        rng.uniform(0.2, 3.0),
                        rng.uniform(0.2, 3.0),
                        rng.uniform(0.2, 3.0),
                    ]
                }
            }),
        },
        26 => {
            let a = rng.uniform(-5.0, 5.0);
            OpIn::EditParam {
                a,
                b: a + rng.uniform(0.5, 4.0),
            }
        }
        27 => {
            let (a, b) = gen_ivl(rng, &c);
            OpIn::Reverse { a, b }
        }
        28 => OpIn::Convert2d,
        29 => {
            let (a, b) = gen_ivl(rng, &c);
            OpIn::Analytic { a, b }
        }
        30 => OpIn::StepInversion { p: rp(rng, 5.0) },
        31 => OpIn::DropFast { p: rp(rng, 5.0) },
        32 => OpIn::IntersectLine,
        33 | 34 => {
            let (a, b) = range_of(&c);
            let sol = match rng.below(3) {
                0 => a + (b - a) * rng.uniform(0.0, 0.01),
                1 => b - (b - a) * rng.uniform(0.0, 0.01),
                _ => gen_t(rng, &c).clamp(a, b),
            };
            OpIn::Snap {
                a,
                b,
                vec: rng.chance(0.85).then(|| [bs(rng), bs(rng), bs(rng)]),
                sol,
                tol: *rng.choose(&[1e-6, 0.01, 0.5, 5.0]).unwrap_or(&0.01),
            }
        }
        35 => match &c {
            CurveIn::Hermite(_) => OpIn::Info,
            _ => OpIn::LengthIfeo { t: gen_t(rng, &c) },
        },
        _ => return gen_static(rng),
    };
    Inputs { curve, op }
}

fn gen_static(rng: &mut Rng) -> Inputs {
    let op = match rng.below(16) {
        0 => {
            let c = gen_curve(rng);
            let (ty, text) = match &c {
                CurveIn::Hermite(h) => (14034, hermite_text(h)),
                CurveIn::Bspline(b) => (14033, bspline_text(b)),
                CurveIn::Circle(_) => (14033, String::new()),
            };
            OpIn::ReadDb {
                curve_type: if rng.chance(0.1) { 14099 } else { ty },
                dim: 3,
                version: 33,
                text,
            }
        }
        1 | 2 => {
            // Hermite spans along a curve, assembled and flushed
            let c = gen_curve(rng);
            let (a, b) = range_of(&c);
            let curve = build(&c).ok();
            let k = 1 + rng.below(6) as usize;
            let mut knots = Vec::new();
            let mut pts = Vec::new();
            let circle = rng.chance(0.4);
            let r = rng.uniform(0.5, 20.0);
            let cen = rp(rng, 5.0);
            let sweep = rng.uniform(0.3, 6.0);
            let mut last: Option<(f64, [Vec3; 2])> = None;
            for i in 0..=k {
                let t = a + (b - a) * (i as f64 / k as f64);
                let pvv = if circle {
                    let th = sweep * (i as f64 / k as f64);
                    let s = sweep / (b - a);
                    [
                        Vec3::new(cen[0] + r * th.cos(), cen[1] + r * th.sin(), cen[2]),
                        Vec3::new(-r * s * th.sin(), r * s * th.cos(), 0.0),
                    ]
                } else {
                    match curve
                        .as_ref()
                        .and_then(|cv| cv.evaluate(&Kernel::default(), t, 1, true, false).ok())
                    {
                        Some(v) => [
                            v.first().copied().unwrap_or(Vec3::ZERO),
                            v.get(1).copied().unwrap_or(Vec3::ZERO),
                        ],
                        None => [Vec3::ZERO, Vec3::ZERO],
                    }
                };
                if let Some((lt, lpv)) = last {
                    let _ = add_to_curve(lpv, pvv, lt, t, &mut pts, &mut knots);
                }
                last = Some((t, pvv));
            }
            OpIn::Flush {
                tol: *rng.choose(&[1e-6, 1e-3, 0.1]).unwrap_or(&1e-6),
                analytics: rng.chance(0.7),
                knots,
                pts: pts.iter().map(|p| p.to_array()).collect(),
            }
        }
        3 => {
            let n = rng.below(3) as usize;
            let mut knots = Vec::new();
            let mut pts = Vec::new();
            let mut t = 0.0;
            if n > 0 {
                knots.push(0.0);
                pts.push(rp(rng, 3.0));
                for _ in 0..n {
                    t += rng.uniform(0.5, 2.0);
                    knots.push(t);
                    for _ in 0..3 {
                        pts.push(rp(rng, 3.0));
                    }
                }
            }
            let start = pts.last().copied().unwrap_or_else(|| rp(rng, 3.0));
            let lt = if rng.chance(0.85) { t } else { t + 0.5 };
            let p0 = if rng.chance(0.85) {
                start
            } else {
                rp(rng, 3.0)
            };
            OpIn::AddToCurve {
                last: [p0, rp(rng, 2.0)],
                curr: [rp(rng, 3.0), rp(rng, 2.0)],
                lt,
                ct: lt + rng.uniform(0.1, 2.0),
                knots,
                pts,
            }
        }
        4 => {
            let init: Vec<f64> = (0..rng.below(16)).map(|_| rng.uniform(-5.0, 5.0)).collect();
            let ops = (0..=rng.below(8))
                .map(|_| match rng.below(3) {
                    0 => StackOp::Push {
                        t: rng.uniform(-3.0, 3.0),
                        p: rp(rng, 3.0),
                        v: rp(rng, 3.0),
                    },
                    1 => StackOp::Pop,
                    _ => StackOp::Look,
                })
                .collect();
            OpIn::Stack { init, ops }
        }
        5 => OpIn::ParamDelta {
            tan: if rng.chance(0.1) {
                [0.0; 3]
            } else {
                rp(rng, 3.0)
            },
            delta: rp(rng, 3.0),
        },
        6 => {
            let z = rng.chance(0.5);
            OpIn::UnitizeTan {
                pvv: [
                    rp(rng, 3.0),
                    if z { [0.0; 3] } else { rp(rng, 3.0) },
                    rp(rng, 3.0),
                ],
                gap: rp(rng, 3.0),
            }
        }
        7 => OpIn::ProjAngle {
            p1: rp(rng, 3.0),
            p2: rp(rng, 3.0),
            c: if rng.chance(0.1) {
                [0.0; 3]
            } else {
                rp(rng, 1.0)
            },
            n: rp(rng, 1.0),
            side: rp(rng, 1.0),
        },
        8 => {
            let mut knots: Vec<f64> = (0..rng.below(8)).map(|_| rng.uniform(0.0, 10.0)).collect();
            knots.sort_by(f64::total_cmp);
            let a = rng.uniform(-1.0, 6.0);
            OpIn::TestParams {
                a,
                b: a + rng.uniform(0.0, 6.0),
                deg: 3,
                knots,
            }
        }
        9 => OpIn::TestStep {
            cur_t: rng.uniform(0.0, 1.0),
            step: rng.uniform(0.01, 1.0),
            cur: [rp(rng, 3.0), rp(rng, 3.0)],
            next: [rp(rng, 3.0), rp(rng, 3.0)],
            chord: *rng.choose(&[0.0, 0.01, 0.1, 1.0]).unwrap_or(&0.0),
            angle: *rng.choose(&[0.0, 5.0, 30.0, 90.0]).unwrap_or(&0.0),
            dsq: *rng.choose(&[0.0, 0.5, 4.0, 40.0]).unwrap_or(&0.0),
        },
        _ => OpIn::Solver(gen_solver(rng)),
    };
    Inputs { curve: None, op }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.curve_p2"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let i = (0..64).map(|_| gen_case(rng)).find(keeps_contract)?;
        serde_json::to_value(i).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
