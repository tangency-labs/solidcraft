//! Runner for unit `curve.curve_p6`: `SmCurve` part 6 (the `CalculateBoundingBox`
//! dispatch and its base body, `IsPlanar`, `DropPoint`, the `GlobalPointSolveSTEP`
//! dispatch, `CurveMaxDistanceBetween`, the simple and global coincidence checkers and
//! `DeformToEndPointTargets`).
//!
//! C++: `solidcore_ffi::units::curve_curve_p6` (upstream `source/SMLib/src/SmCurve.cpp`,
//! called through `SmCurve*` on B-spline, Hermite and line curves). Rust:
//! `solidcore::curve::curve::p6`.
//!
//! A case is one `op` with its curves (`{"bspline": {dim, oob, cpts, p, u}}` with control
//! points as stored `[x, y, z, w]`, `{"hermite": {dim, p1, d1, p2, d2}}`, `{"circle": ..}`
//! (a quadratic B-spline arc) or `{"line": {p, v, domain, scale, inside_out}}`, as in
//! unit `curve.curve_p3`) and parameters. Both sides produce the flat record documented
//! in the ffi module; the runner reports its first entry as the `status` int (`0` =
//! `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a Rust
//! error upstream cannot produce) and the rest as the `out` values.
//!
//! Deferred cases (unit `curve.curve_p5`): the precise (`nsamp = 0`) gap refinement of
//! `CurveMaxDistanceBetween` (`SmFindCCNormalENFO::Evaluate` behind `LocalCurveSolve`),
//! the split recombination of `GlobalCoincidenceChecker` once a piece has a solution
//! (`sm_IsPointSolutionAcrossClosedSeamBoundary`) and the global solves that reach
//! `LocalPropertyAnalysis` or `SmCurve::IsArc`. The base `CalculateBoundingBox` body is
//! only reached by curve kinds without an override (none of the kinds here: B-spline,
//! line and Hermite all override it). Global point solves name a curve handle carrying
//! the curve's natural interval (upstream's `this`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::{Curve, CurveKind};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::global_solver::{SolutionRequested, SolverOperation};
use solidcore::geom::solution_array::{ObjectKind, Solution, SolutionObject, SolutionType};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BoundingBoxRequest};
use solidcore::nurbs::b_spline_curve_n_lib::InValueType;
use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::SolidError;
use solidcore_ffi::units::curve_curve_p6::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `curve.curve_p6` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

const UNDEF: f64 = 1.23456e-61;

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
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    BBox {
        curve: CurveIn,
        ivl: [f64; 2],
        normal: bool,
        pseudo: bool,
        polar: bool,
        #[serde(default = "yes")]
        expand: bool,
    },
    IsPlanar {
        curve: CurveIn,
        tol: f64,
    },
    DropPoint {
        curve: CurveIn,
        ivl: [f64; 2],
        p: [f64; 3],
        vec: Option<[f64; 3]>,
        tol: f64,
        guess: Option<f64>,
        sop: i32,
    },
    GpsStep {
        curve: CurveIn,
        ivl: [f64; 2],
        sop: i32,
        p: [f64; 3],
        tol: f64,
        target: Option<f64>,
        req: i32,
    },
    MaxDist {
        c1: CurveIn,
        ivl: [f64; 2],
        /// `None`: the other curve is `c1` itself.
        c2: Option<CurveIn>,
        s: f64,
        e: f64,
        nsamp: u64,
        max: Option<f64>,
    },
    Simple {
        c1: CurveIn,
        ivl1: [f64; 2],
        c2: CurveIn,
        ivl2: [f64; 2],
        tol: f64,
        pb1: bool,
        pb2: bool,
    },
    Global {
        c1: CurveIn,
        ivl1: [f64; 2],
        c2: CurveIn,
        ivl2: [f64; 2],
        tol: f64,
        #[serde(default = "yes")]
        snap: bool,
    },
    Deform {
        curve: CurveIn,
        s: Option<[f64; 3]>,
        e: Option<[f64; 3]>,
        want: bool,
        st: Option<[f64; 3]>,
        st_type: u8,
        et: Option<[f64; 3]>,
        et_type: u8,
    },
}

fn yes() -> bool {
    true
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
        OpIn::BBox { .. } => Op::BBox,
        OpIn::IsPlanar { .. } => Op::IsPlanar,
        OpIn::DropPoint { .. } => Op::DropPoint,
        OpIn::GpsStep { .. } => Op::GpsStep,
        OpIn::MaxDist { .. } => Op::MaxDist,
        OpIn::Simple { .. } => Op::Simple,
        OpIn::Global { .. } => Op::Global,
        OpIn::Deform { .. } => Op::Deform,
    }
}

fn push_opt3(v: &mut Vec<f64>, x: Option<[f64; 3]>) {
    v.push(b(x.is_some()));
    v.extend_from_slice(&x.unwrap_or([0.0; 3]));
}

fn push_opt(v: &mut Vec<f64>, x: Option<f64>) {
    v.extend([b(x.is_some()), x.unwrap_or(0.0)]);
}

fn cpp_input(op: &OpIn) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut v = Vec::new();
    match op {
        OpIn::BBox {
            curve,
            ivl,
            normal,
            pseudo,
            polar,
            expand,
        } => {
            curve_block(&mut v, curve)?;
            v.extend_from_slice(ivl);
            v.extend([b(*normal), b(*pseudo), b(*polar), b(*expand)]);
        }
        OpIn::IsPlanar { curve, tol } => {
            curve_block(&mut v, curve)?;
            v.push(*tol);
        }
        OpIn::DropPoint {
            curve,
            ivl,
            p,
            vec,
            tol,
            guess,
            sop,
        } => {
            curve_block(&mut v, curve)?;
            v.extend_from_slice(ivl);
            v.extend_from_slice(p);
            push_opt3(&mut v, *vec);
            v.push(*tol);
            push_opt(&mut v, *guess);
            v.push(f64::from(*sop));
        }
        OpIn::GpsStep {
            curve,
            ivl,
            sop,
            p,
            tol,
            target,
            req,
        } => {
            curve_block(&mut v, curve)?;
            v.extend_from_slice(ivl);
            v.push(f64::from(*sop));
            v.extend_from_slice(p);
            v.push(*tol);
            push_opt(&mut v, *target);
            v.push(f64::from(*req));
        }
        OpIn::MaxDist {
            c1,
            ivl,
            c2,
            s,
            e,
            nsamp,
            max,
        } => {
            curve_block(&mut v, c1)?;
            v.extend_from_slice(ivl);
            v.push(b(c2.is_none()));
            if let Some(c2) = c2 {
                curve_block(&mut v, c2)?;
            }
            v.extend([*s, *e, *nsamp as f64]);
            v.extend([b(max.is_some()), max.unwrap_or(1.0)]);
        }
        OpIn::Simple {
            c1,
            ivl1,
            c2,
            ivl2,
            tol,
            pb1,
            pb2,
        } => {
            curve_block(&mut v, c1)?;
            v.extend_from_slice(ivl1);
            curve_block(&mut v, c2)?;
            v.extend_from_slice(ivl2);
            v.extend([*tol, b(*pb1), b(*pb2)]);
        }
        OpIn::Global {
            c1,
            ivl1,
            c2,
            ivl2,
            tol,
            snap,
        } => {
            curve_block(&mut v, c1)?;
            v.extend_from_slice(ivl1);
            curve_block(&mut v, c2)?;
            v.extend_from_slice(ivl2);
            v.extend([*tol, b(*snap)]);
        }
        OpIn::Deform {
            curve,
            s,
            e,
            want,
            st,
            st_type,
            et,
            et_type,
        } => {
            curve_block(&mut v, curve)?;
            push_opt3(&mut v, *s);
            push_opt3(&mut v, *e);
            v.push(b(*want));
            push_opt3(&mut v, *st);
            v.push(f64::from(*st_type));
            push_opt3(&mut v, *et);
            v.push(f64::from(*et_type));
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

/// A curve handle: its id (`0` this curve, `1` the other) and natural interval.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Obj(i64, Extent1d);

impl SolutionObject for Obj {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }

    fn natural_interval(&self) -> Option<Extent1d> {
        Some(self.1)
    }
}

/// A solution record (see the ffi module docs); `id` maps the objects.
fn put_sol<O: Copy>(o: &mut Vec<f64>, s: &Solution<O>, id: impl Fn(Option<O>) -> f64) {
    let second = |e: &solidcore::geom::solution_array::SolutionEnd| {
        if s.num_variables > 1 {
            e.parameters[1]
        } else {
            0.0
        }
    };
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
        second(&s.start),
    ]);
    if s.solution_type == SolutionType::RangeOfValues {
        o.extend([
            s.end.solution_value,
            s.end.parameters[0],
            second(&s.end),
        ]);
    }
}

fn obj_id(x: Option<Obj>) -> f64 {
    x.map_or(-1.0, |v| v.0 as f64)
}

fn sop_of(code: i32) -> Result<SolverOperation, HarnessError> {
    SolverOperation::from_code(code).ok_or_else(|| harness("solver operation"))
}

fn ivl_of(i: [f64; 2]) -> Extent1d {
    Extent1d::from_min_max(i[0], i[1])
}

fn in_value(t: u8) -> Result<InValueType, HarnessError> {
    match t {
        0 => Ok(InValueType::Specified),
        1 => Ok(InValueType::Same),
        2 => Ok(InValueType::Unconstrained),
        _ => Err(harness("tangent type")),
    }
}

/// The curve state record (see the ffi module docs).
fn put_curve(k: &Kernel, o: &mut Vec<f64>, c: &Curve) -> Result<(), Fail> {
    let kind = match c.kind() {
        CurveKind::BSpline(_) => 0.0,
        CurveKind::Line(_) => 2.0,
        _ => 1.0,
    };
    o.push(kind);
    let ivl = c.natural_interval();
    o.extend([ivl.min(), ivl.max()]);
    for i in 0..=6 {
        let p = match c.evaluate_point(k, ivl.evaluate(f64::from(i) / 6.0)) {
            Ok(p) => p,
            Err(e) if e.is_unported() => return Err(e.into()),
            Err(_) => Vec3::new(UNDEF, UNDEF, UNDEF),
        };
        pv(o, p);
    }
    if let CurveKind::BSpline(bs) = c.kind() {
        let nurb = bs.gw_nurb().ok_or(Fail::Status(-1.0))?;
        o.push(nurb.cpts().len() as f64);
        for p in nurb.cpts() {
            o.extend([p.x, p.y, p.z, p.w]);
        }
        o.push(nurb.knots().len() as f64);
        o.extend_from_slice(nurb.knots());
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn rust(op: &OpIn) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut o: Vec<f64> = Vec::new();
    match op {
        OpIn::BBox {
            curve,
            ivl,
            normal,
            pseudo,
            polar,
            expand,
        } => {
            let c = build(curve)?;
            let req = BoundingBoxRequest {
                normal: *normal,
                pseudo: *pseudo,
                polar: *polar,
            };
            let bx = tri!(o, c.calculate_bounding_box(&Kernel::default(), &ivl_of(*ivl), req, *expand));
            o.push(0.0);
            if let Some(nb) = bx.normal {
                o.extend((0..3).map(|i| nb.min_at(i)));
                o.extend((0..3).map(|i| nb.max_at(i)));
            }
            if let Some(pb) = bx.pseudo {
                for v in pb.basis() {
                    pv(&mut o, *v);
                }
                for iv in pb.intervals() {
                    o.extend([iv.min(), iv.max()]);
                }
            }
            if let Some(pl) = bx.polar {
                o.push(f64::from(pl.state().code()));
                for v in pl.basis() {
                    pv(&mut o, *v);
                }
                let d = pl.domain();
                o.extend([d.u_min(), d.v_min(), d.u_max(), d.v_max()]);
            }
        }
        OpIn::IsPlanar { curve, tol } => {
            let c = build(curve)?;
            let r = tri!(o, c.is_planar(&Kernel::default(), *tol));
            o.extend([0.0, b(r.is_planar)]);
            pv(&mut o, r.point);
            pv(&mut o, r.normal);
            o.push(r.deviation);
        }
        OpIn::DropPoint {
            curve,
            ivl,
            p,
            vec,
            tol,
            guess,
            sop,
        } => {
            let c = build(curve)?;
            let d = tri!(
                o,
                c.drop_point(
                    &k,
                    &ivl_of(*ivl),
                    v3(*p),
                    vec.map(v3),
                    *tol,
                    *guess,
                    sop_of(*sop)?
                )
            );
            o.extend([0.0, b(d.success), d.parameter, d.distance]);
        }
        OpIn::GpsStep {
            curve,
            ivl,
            sop,
            p,
            tol,
            target,
            req,
        } => {
            let mut c = build(curve)?;
            let requested = SolutionRequested::from_code(*req).ok_or_else(|| harness("req"))?;
            let this = Obj(0, c.natural_interval());
            let sols = tri!(
                o,
                c.global_point_solve_step(
                    &k,
                    &ivl_of(*ivl),
                    sop_of(*sop)?,
                    v3(*p),
                    *tol,
                    *target,
                    None,
                    requested,
                    Some(this),
                    Some(Obj(2, ivl_of(*ivl))),
                )
            );
            o.extend([0.0, sols.len() as f64]);
            for s in &sols {
                put_sol(&mut o, s, obj_id);
            }
        }
        OpIn::MaxDist {
            c1,
            ivl,
            c2,
            s,
            e,
            nsamp,
            max,
        } => {
            // the FFI rejects more than `ffi::MAX_SAMPLES` samples; upstream (and the port)
            // would loop that many times, so a larger count is not a case for Rust either
            if *nsamp > ffi::MAX_SAMPLES as u64 {
                return Err(HarnessError::new(format!("{nsamp} samples")));
            }
            let a = build(c1)?;
            let other = match c2 {
                Some(c2) => Some(build(c2)?),
                None => None,
            };
            let r = tri!(
                o,
                a.curve_max_distance_between(
                    &k,
                    &ivl_of(*ivl),
                    other.as_ref().unwrap_or(&a),
                    *s,
                    *e,
                    *nsamp,
                    *max,
                )
            );
            o.extend([
                0.0,
                r.distance,
                r.this_t,
                r.other_t,
                b(r.incorrect_dir.is_some()),
                b(r.incorrect_dir.unwrap_or(false)),
            ]);
        }
        OpIn::Simple {
            c1,
            ivl1,
            c2,
            ivl2,
            tol,
            pb1,
            pb2,
        } => {
            let a = build(c1)?;
            let bb = build(c2)?;
            let (i1, i2) = (ivl_of(*ivl1), ivl_of(*ivl2));
            let req = BoundingBoxRequest {
                normal: false,
                pseudo: true,
                polar: false,
            };
            let box1 = if *pb1 {
                tri!(o, a.calculate_bounding_box(&Kernel::default(), &i1, req, true)).pseudo
            } else {
                None
            };
            let box2 = if *pb2 {
                tri!(o, bb.calculate_bounding_box(&Kernel::default(), &i2, req, true)).pseudo
            } else {
                None
            };
            let sols = tri!(
                o,
                a.simple_coincidence_checker(
                    &k,
                    &i1,
                    &bb,
                    &i2,
                    *tol,
                    box1.as_ref(),
                    box2.as_ref(),
                    [
                        Some(Obj(0, a.natural_interval())),
                        Some(Obj(1, bb.natural_interval())),
                    ],
                )
            );
            o.extend([0.0, sols.len() as f64]);
            for s in &sols {
                put_sol(&mut o, s, obj_id);
            }
        }
        OpIn::Global {
            c1,
            ivl1,
            c2,
            ivl2,
            tol,
            snap,
        } => {
            let a = build(c1)?;
            let bb = build(c2)?;
            let r = tri!(
                o,
                a.global_coincidence_checker(
                    &k,
                    &ivl_of(*ivl1),
                    &bb,
                    &ivl_of(*ivl2),
                    *tol,
                    *snap,
                    [
                        Some(Obj(0, a.natural_interval())),
                        Some(Obj(1, bb.natural_interval())),
                    ],
                )
            );
            o.extend([0.0, b(r.additional_work_needed), r.solutions.len() as f64]);
            for s in &r.solutions {
                put_sol(&mut o, s, obj_id);
            }
        }
        OpIn::Deform {
            curve,
            s,
            e,
            want,
            st,
            st_type,
            et,
            et_type,
        } => {
            let mut c = build(curve)?;
            let nc = tri!(
                o,
                c.deform_to_end_point_targets(
                    &k,
                    s.map(v3),
                    e.map(v3),
                    *want,
                    st.map(v3),
                    in_value(*st_type)?,
                    et.map(v3),
                    in_value(*et_type)?,
                )
            );
            o.extend([0.0, b(nc.is_some())]);
            let state = put_curve(&k, &mut o, nc.as_ref().unwrap_or(&c));
            match state {
                Ok(()) => {}
                Err(Fail::Deferred(d)) => return Ok(*d),
                Err(Fail::Status(_)) => return Err(harness("B-spline without NURB")),
            }
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
        0..=2 => gen_circle(rng),
        3..=6 => CurveIn::Line(gen_line(rng)),
        7..=14 => CurveIn::Bspline(gen_bspline(rng)),
        _ => CurveIn::Hermite(gen_hermite(rng)),
    };
    resolve(&c).unwrap_or(c)
}

/// A planar curve (B-spline in a random plane, line, planar Hermite or arc).
fn gen_planar_curve(rng: &mut Rng) -> CurveIn {
    match rng.below(4) {
        0 => gen_curve(rng),
        1 => {
            let mut h = gen_hermite(rng);
            h.dim = 3;
            let n = gen_dir(rng);
            // project the data into the plane through the origin normal to n
            for v in [&mut h.p1, &mut h.d1, &mut h.p2, &mut h.d2] {
                let d = v[0] * n[0] + v[1] * n[1] + v[2] * n[2];
                let nn = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
                *v = [
                    v[0] - d * n[0] / nn,
                    v[1] - d * n[1] / nn,
                    v[2] - d * n[2] / nn,
                ];
            }
            CurveIn::Hermite(h)
        }
        _ => {
            let mut bs = gen_bspline(rng);
            let jitter = *rng.choose(&[0.0, 0.0, 1e-9, 1e-4, 0.1]).unwrap_or(&0.0);
            for c in &mut bs.cpts {
                if c[2] != NOZ {
                    let w = if c[3] == NOW { 1.0 } else { c[3] };
                    c[2] = w * (1.5 + rng.uniform(-jitter, jitter));
                }
            }
            CurveIn::Bspline(bs)
        }
    }
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

/// An increasing interval of `c`: mostly its natural one or a sub-interval.
fn gen_ivl(rng: &mut Rng, c: &CurveIn) -> [f64; 2] {
    let (a, bb) = range_of(c);
    if rng.chance(0.5) {
        return [a, bb];
    }
    for _ in 0..16 {
        let x = a + (bb - a) * rng.uniform(-0.1, 1.1);
        let y = a + (bb - a) * rng.uniform(-0.1, 1.1);
        if (x - y).abs() > 1e-3 * (bb - a) {
            return [x.min(y), x.max(y)];
        }
    }
    [a, bb]
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

/// The point of `c` at `t` (Rust evaluation, used only to place test points).
fn point_at(k: &Kernel, c: &CurveIn, t: f64) -> [f64; 3] {
    build(c)
        .ok()
        .and_then(|cc| cc.evaluate_point(k, t).ok())
        .map_or([0.0; 3], |p| [p.x, p.y, p.z])
}

/// `c` moved by `d` (control points / line point / Hermite points).
fn translated(c: &CurveIn, d: [f64; 3]) -> CurveIn {
    match c {
        CurveIn::Bspline(bs) => {
            let mut bs = bs.clone();
            for p in &mut bs.cpts {
                let w = if p[3] == NOW { 1.0 } else { p[3] };
                p[0] += w * d[0];
                p[1] += w * d[1];
                if p[2] != NOZ {
                    p[2] += w * d[2];
                }
            }
            CurveIn::Bspline(bs)
        }
        CurveIn::Hermite(h) => {
            let mut h = h.clone();
            h.p1 = add(h.p1, d);
            h.p2 = add(h.p2, d);
            if h.dim == 2 {
                h.p1[2] = 0.0;
                h.p2[2] = 0.0;
            }
            CurveIn::Hermite(h)
        }
        CurveIn::Line(l) => {
            let mut l = l.clone();
            l.p = add(l.p, d);
            CurveIn::Line(l)
        }
        CurveIn::Circle(ci) => {
            let mut ci = ci.clone();
            ci.center = add(ci.center, d);
            CurveIn::Circle(ci)
        }
    }
}

/// A second curve near `c`: an identical copy, a slightly moved copy, the same line
/// reparameterised, or an unrelated curve.
fn gen_partner(k: &Kernel, rng: &mut Rng, c: &CurveIn) -> CurveIn {
    match rng.below(8) {
        0..=2 => c.clone(),
        3 | 4 => {
            let off = *rng.choose(&[1e-9, 1e-7, 1e-5, 1e-3, 0.1]).unwrap_or(&1e-7);
            translated(c, scale3(gen_dir(rng), off))
        }
        5 => {
            // a line on the same carrier with another parameterisation
            let (a, bb) = range_of(c);
            let p = point_at(k, c, a);
            let q = point_at(k, c, bb);
            let d = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
            if rng.chance(0.5) {
                CurveIn::Line(LineIn {
                    p,
                    v: d,
                    domain: [0.0, 1.0],
                    scale: 1.0,
                    inside_out: rng.chance(0.5),
                })
            } else {
                CurveIn::Line(LineIn {
                    p: q,
                    v: scale3(d, -1.0),
                    domain: [rng.uniform(-1.0, 1.0), rng.uniform(1.5, 3.0)],
                    scale: 1.0,
                    inside_out: false,
                })
            }
        }
        _ => gen_curve(rng),
    }
}

/// A second interval for a partner curve.
fn gen_partner_ivl(rng: &mut Rng, c: &CurveIn, own: [f64; 2]) -> [f64; 2] {
    let (a, bb) = range_of(c);
    match rng.below(4) {
        0 => [a, bb],
        1 if own[0] >= a && own[1] <= bb && own[0] < own[1] => own,
        _ => gen_ivl(rng, c),
    }
}

fn gen_tol(rng: &mut Rng) -> f64 {
    *rng.choose(&[1e-6, 1e-5, 1e-4, 1e-3, 1e-2, 0.5])
        .unwrap_or(&1e-5)
}

fn gen_bbox(rng: &mut Rng) -> OpIn {
    let curve = if rng.chance(0.15) {
        gen_closed_curve(rng)
    } else {
        gen_curve(rng)
    };
    let ivl = gen_ivl(rng, &curve);
    let mut normal = rng.chance(0.7);
    let pseudo = rng.chance(0.6);
    let polar = rng.chance(0.5);
    if !normal && !pseudo && !polar && rng.chance(0.8) {
        normal = true;
    }
    OpIn::BBox {
        curve,
        ivl,
        normal,
        pseudo,
        polar,
        expand: rng.chance(0.7),
    }
}

fn gen_is_planar(rng: &mut Rng) -> OpIn {
    OpIn::IsPlanar {
        curve: gen_planar_curve(rng),
        tol: *rng
            .choose(&[1e-12, 1e-12, 1e-9, 1e-6, 1e-3, 0.5])
            .unwrap_or(&1e-12),
    }
}

fn gen_drop_point(k: &Kernel, rng: &mut Rng) -> OpIn {
    let curve = gen_curve(rng);
    let ivl = gen_ivl(rng, &curve);
    let t = gen_t(rng, &curve);
    let base = point_at(k, &curve, t);
    let off = *rng.choose(&[0.0, 1e-8, 1e-4, 0.01, 0.5, 3.0]).unwrap_or(&0.0);
    let p = add(base, scale3(gen_dir(rng), off));
    let tol = gen_tol(rng);
    let sop = *rng.choose(&[3, 3, 3, 0, 0, 5, 5, 4, 6, 1]).unwrap_or(&3);
    let vec = if rng.chance(0.4) {
        Some(if rng.chance(0.3) {
            gen_dir(rng)
        } else {
            // an end tangent pointing into the curve
            let (a, bb) = range_of(&curve);
            let at_start = rng.chance(0.5);
            let (t0, t1) = if at_start {
                (a, a + 1e-3 * (bb - a))
            } else {
                (bb, bb - 1e-3 * (bb - a))
            };
            let p0 = point_at(k, &curve, t0);
            let p1 = point_at(k, &curve, t1);
            [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]]
        })
    } else {
        None
    };
    let guess = if rng.chance(0.85) {
        Some(match rng.below(3) {
            0 => t,
            1 => gen_t(rng, &curve),
            _ => t + rng.uniform(-0.1, 0.1) * (ivl[1] - ivl[0]),
        })
    } else {
        None
    };
    OpIn::DropPoint {
        curve,
        ivl,
        p,
        vec,
        tol,
        guess,
        sop,
    }
}

fn gen_gps_step(k: &Kernel, rng: &mut Rng) -> OpIn {
    let curve = if rng.chance(0.8) {
        CurveIn::Line(gen_line(rng))
    } else {
        gen_curve(rng)
    };
    let ivl = gen_ivl(rng, &curve);
    let t = gen_t(rng, &curve);
    let p = add(
        point_at(k, &curve, t),
        scale3(gen_dir(rng), *rng.choose(&[0.0, 1e-6, 0.3, 4.0]).unwrap_or(&0.0)),
    );
    OpIn::GpsStep {
        curve,
        ivl,
        sop: *rng.choose(&[0, 3, 4, 5]).unwrap_or(&3),
        p,
        tol: gen_tol(rng),
        target: if rng.chance(0.3) {
            Some(rng.uniform(0.0, 5.0))
        } else {
            None
        },
        req: i32::from(rng.chance(0.5)),
    }
}

fn gen_max_dist(k: &Kernel, rng: &mut Rng) -> OpIn {
    let c1 = gen_curve(rng);
    let ivl = gen_ivl(rng, &c1);
    let same = rng.chance(0.1);
    let c2 = if same { None } else { Some(gen_partner(k, rng, &c1)) };
    let other = c2.as_ref().unwrap_or(&c1);
    let oi = if same && rng.chance(0.6) {
        ivl
    } else {
        gen_partner_ivl(rng, other, ivl)
    };
    let (s, e) = if rng.chance(0.25) {
        (oi[1], oi[0])
    } else {
        (oi[0], oi[1])
    };
    let nsamp = *rng
        .choose(&[2u64, 3, 5, 6, 8, 10, 20, 0, 4])
        .unwrap_or(&5);
    OpIn::MaxDist {
        c1,
        ivl,
        c2,
        s,
        e,
        nsamp,
        max: if rng.chance(0.4) {
            Some(gen_tol(rng))
        } else {
            None
        },
    }
}

fn gen_simple(k: &Kernel, rng: &mut Rng) -> OpIn {
    let c1 = gen_curve(rng);
    let ivl1 = gen_ivl(rng, &c1);
    let c2 = gen_partner(k, rng, &c1);
    let ivl2 = gen_partner_ivl(rng, &c2, ivl1);
    OpIn::Simple {
        c1,
        ivl1,
        c2,
        ivl2,
        tol: gen_tol(rng),
        pb1: rng.chance(0.3),
        pb2: rng.chance(0.3),
    }
}

fn gen_global(k: &Kernel, rng: &mut Rng) -> OpIn {
    let c1 = if rng.chance(0.2) {
        gen_closed_curve(rng)
    } else {
        gen_curve(rng)
    };
    let ivl1 = gen_ivl(rng, &c1);
    let c2 = match rng.below(6) {
        // far away: disjoint boxes
        0..=2 => translated(&gen_curve(rng), [300.0, -200.0, 50.0]),
        // degenerate other (a point-sized Hermite)
        3 => {
            let p = rp(rng, 5.0);
            CurveIn::Hermite(HermiteIn {
                dim: 3,
                p1: p,
                d1: [0.0; 3],
                p2: p,
                d2: [0.0; 3],
            })
        }
        _ => gen_partner(k, rng, &c1),
    };
    let ivl2 = gen_partner_ivl(rng, &c2, ivl1);
    let (c1, ivl1, c2, ivl2) = if rng.chance(0.3) {
        (c2, ivl2, c1, ivl1)
    } else {
        (c1, ivl1, c2, ivl2)
    };
    OpIn::Global {
        c1,
        ivl1,
        c2,
        ivl2,
        tol: gen_tol(rng),
        snap: rng.chance(0.7),
    }
}

fn gen_deform(k: &Kernel, rng: &mut Rng) -> OpIn {
    let curve = match rng.below(10) {
        0..=4 => {
            // mostly odd-degree B-splines, as the B-spline deformation expects
            let mut bs = gen_bspline(rng);
            if bs.dim == 2 && rng.chance(0.5) {
                bs = gen_bspline(rng);
            }
            CurveIn::Bspline(bs)
        }
        5..=7 => CurveIn::Line(gen_line(rng)),
        8 => gen_circle(rng),
        _ => CurveIn::Hermite(gen_hermite(rng)),
    };
    let curve = resolve(&curve).unwrap_or(curve);
    let tan_type = |rng: &mut Rng| *rng.choose(&[0u8, 1, 1, 2, 2]).unwrap_or(&1);
    let st_type = tan_type(rng);
    let et_type = tan_type(rng);
    let (a, bb) = range_of(&curve);
    let ps = point_at(k, &curve, a);
    let pe = point_at(k, &curve, bb);
    OpIn::Deform {
        s: if rng.chance(0.7) {
            Some(add(ps, rp(rng, 1.0)))
        } else {
            None
        },
        e: if rng.chance(0.6) {
            Some(add(pe, rp(rng, 1.0)))
        } else {
            None
        },
        want: rng.chance(0.85),
        st: if st_type == 0 || rng.chance(0.2) {
            Some(gen_dir(rng))
        } else {
            None
        },
        st_type,
        et: if et_type == 0 || rng.chance(0.2) {
            Some(gen_dir(rng))
        } else {
            None
        },
        et_type,
        curve,
    }
}

fn gen_case(k: &Kernel, rng: &mut Rng) -> OpIn {
    match rng.below(100) {
        0..=11 => gen_bbox(rng),
        12..=21 => gen_is_planar(rng),
        22..=37 => gen_drop_point(k, rng),
        38..=45 => gen_gps_step(k, rng),
        46..=63 => gen_max_dist(k, rng),
        64..=79 => gen_simple(k, rng),
        80..=89 => gen_global(k, rng),
        _ => gen_deform(k, rng),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.curve_p6"
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
        let k = &Kernel::default();
        let mut rng = Rng::new(7);
        let mut stats: BTreeMap<String, [usize; 4]> = BTreeMap::new();
        for _ in 0..2000 {
            let op = gen_case(k, &mut rng);
            let name = serde_json::to_value(&op).unwrap()["op"]
                .as_str()
                .unwrap()
                .to_string();
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
            eprintln!(
                "{k}: total {} ok {} err {} deferred/harness {}",
                v[0], v[1], v[2], v[3]
            );
        }
        assert_eq!(stats.len(), 8);
    }
}
