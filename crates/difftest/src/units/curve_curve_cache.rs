//! Runner for unit `curve.curve_cache`: `SmCurveCache` over B-spline and Hermite
//! curves (tessellation into Bezier spans, the decomposition tree, analytic
//! conversion, type queries).
//!
//! C++: `solidcore_ffi::units::curve_curve_cache` (upstream
//! `source/SMLib/src/SmCurveCache.cpp`). Rust: `solidcore::curve::curve_cache`.
//!
//! A case is a `curve` (`{"bspline": {dim, cpts, p, u}}` with control points as stored
//! `[x, y, z, w]`, `{"hermite": {dim, p1, d1, p2, d2}}`, or `{"circle": ...}`, a
//! quadratic arc built by the Rust `CreateCircleSegment` and handed to both sides as
//! B-spline data), the cache interval `a`, `b`, the constructor options and an `op`.
//! Both sides produce the flat record documented in the ffi module; the runner reports
//! its first entry as the `status` int (`0` = `SM_SUCCESS`, else the `SmStatus`; `1007`
//! for a Rust `InvalidInput`, `-1` for a Rust error upstream cannot produce) and the
//! rest as the `out` values.
//!
//! Deferred cases: the analytic conversion of lines and arcs (`SmLine`, `SmCircle`,
//! `SmCurve::IsArc`). Every run has at least [`MIN_GENERATED`] generated cases;
//! generated inputs the ffi wrapper rejects are redrawn.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::Curve;
use solidcore::curve::curve_cache::{BezierAux1d, CurveCache, CurveCacheOptions};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::tree::{CurveSpanRef, NodeData, NodeId, Tree};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, NurbCircleParam};
use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::SolidError;
use solidcore_ffi::units::curve_curve_cache::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::v3;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// `curve.curve_cache` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BSplineIn {
    dim: u64,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
struct OptsIn {
    full: bool,
    chord: f64,
    angle: f64,
    offaxis: f64,
    minseg: u64,
    aux: bool,
    max3d: f64,
    ratio: f64,
}

impl Default for OptsIn {
    fn default() -> Self {
        Self {
            full: false,
            chord: 0.0,
            angle: 0.0,
            offaxis: 0.0,
            minseg: 0,
            aux: false,
            max3d: 0.0,
            ratio: 0.0001,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum OpIn {
    Tessellate,
    Analytic,
    Type,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    curve: CurveIn,
    a: f64,
    b: f64,
    #[serde(default)]
    opts: OptsIn,
    op: OpIn,
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
        NurbCircleParam::Quadratic,
    )
    .map_err(|e| harness(format!("circle: {e}")))?;
    let cur = arc.gw_nurb().ok_or_else(|| harness("circle without NURB"))?;
    Ok(CurveIn::Bspline(BSplineIn {
        dim: 3,
        cpts: cur.cpts().iter().map(|p| [p.x, p.y, p.z, p.w]).collect(),
        p: cur.degree(),
        u: cur.knots().to_vec(),
    }))
}

fn push3(v: &mut Vec<f64>, p: [f64; 3]) {
    v.extend(p);
}

fn b(x: bool) -> f64 {
    if x {
        1.0
    } else {
        0.0
    }
}

fn op_of(op: OpIn) -> Op {
    match op {
        OpIn::Tessellate => Op::Tessellate,
        OpIn::Analytic => Op::Analytic,
        OpIn::Type => Op::Type,
    }
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut v = Vec::new();
    match resolve(&i.curve)? {
        CurveIn::Bspline(c) => {
            let n = c.cpts.len().checked_sub(1).ok_or_else(|| harness("no points"))?;
            let m = c.u.len().checked_sub(1).ok_or_else(|| harness("no knots"))?;
            v.extend([0.0, c.dim as f64, n as f64, c.p as f64, m as f64]);
            for p in &c.cpts {
                v.extend(p);
            }
            v.extend(&c.u);
        }
        CurveIn::Hermite(h) => {
            v.extend([1.0, h.dim as f64]);
            for p in [h.p1, h.d1, h.p2, h.d2] {
                push3(&mut v, p);
            }
        }
        CurveIn::Circle(_) => return Err(harness("unresolved circle")),
    }
    let o = &i.opts;
    v.extend([
        i.a,
        i.b,
        b(o.full),
        o.chord,
        o.angle,
        o.offaxis,
        o.minseg as f64,
        b(o.aux),
        o.max3d,
        o.ratio,
    ]);
    Ok((op_of(i.op), v))
}

fn to_output(flat: &[f64]) -> Output {
    let status = flat.first().copied().unwrap_or(f64::NAN);
    let rest = flat.get(1..).unwrap_or(&[]).to_vec();
    Output::new().ints("status", vec![status as i64]).values("out", rest)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, input) = cpp_input(i)?;
    match ffi::eval(op, &input) {
        Ok(flat) => Ok(to_output(&flat)),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- Rust side --------------------------------------------------------------------

fn build(c: &CurveIn) -> Result<Curve, HarnessError> {
    match &resolve(c)? {
        CurveIn::Bspline(bs) => {
            let n = bs.cpts.len().checked_sub(1).ok_or_else(|| harness("no points"))?;
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
            Ok(Curve::from(BSplineCurve::from_nurb(cur, bs.dim, false)))
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

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

fn pv(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_aux(o: &mut Vec<f64>, a: &BezierAux1d) {
    o.extend([
        a.left as f64,
        a.right as f64,
        f64::from(a.conts[0] as i32),
        f64::from(a.conts[1] as i32),
        a.max_length,
        a.max_chord_height_squared,
        a.max_turning_angle_deg,
        a.interval.min(),
        a.interval.max(),
    ]);
    pv(o, a.ends[0]);
    pv(o, a.ends[1]);
}

fn tree_out(o: &mut Vec<f64>, tree: &Tree<u32>) -> Result<(), HarnessError> {
    let he = |e: SolidError| harness(e.to_string());
    // (a map, not `order.contains`: a fuzzed tessellation builds millions of nodes)
    let mut order: Vec<NodeId> = Vec::new();
    let mut index: BTreeMap<NodeId, usize> = BTreeMap::new();
    let mut stack: Vec<NodeId> = tree.top_node().into_iter().collect();
    while let Some(id) = stack.pop() {
        if index.contains_key(&id) {
            continue;
        }
        index.insert(id, order.len());
        order.push(id);
        let n = tree.node(id).map_err(he)?;
        if let Some(c) = n.child2 {
            stack.push(c);
        }
        if let Some(c) = n.child1 {
            stack.push(c);
        }
    }
    let idx_of = |p: Option<NodeId>| match p {
        None => -1.0,
        Some(p) => index.get(&p).map_or(-2.0, |&i| i as f64),
    };
    o.push(order.len() as f64);
    for &id in &order {
        let n = tree.node(id).map_err(he)?;
        o.extend([
            f64::from(n.aux_type.code()),
            f64::from(n.geom_type.code()),
            idx_of(n.parent),
            idx_of(n.child1),
            idx_of(n.child2),
        ]);
        pv(o, n.bbox.min());
        pv(o, n.bbox.max());
        match n.data {
            NodeData::None => o.push(0.0),
            NodeData::Curve(CurveSpanRef::Span(i)) => o.extend([1.0, i as f64]),
            NodeData::Curve(CurveSpanRef::Parent(i)) => {
                o.extend([2.0, i as f64]);
                let a = tree
                    .curve_parents()
                    .get(i)
                    .ok_or_else(|| harness("parent index out of range"))?;
                put_aux(o, a);
            }
            NodeData::ObjectList(_) | NodeData::Surface(_) => o.push(3.0),
        }
    }
    Ok(())
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let curve = build(&i.curve)?;
    let opts = CurveCacheOptions {
        make_full_tree: i.opts.full,
        chord_height_tolerance: i.opts.chord,
        angle_tol_deg: i.opts.angle,
        off_axis_tolerance: i.opts.offaxis,
        min_segments: i.opts.minseg,
        compute_aux_tree_data: i.opts.aux,
        max_3d_distance: i.opts.max3d,
        min_parametric_ratio: i.opts.ratio,
    };
    let mut cc: CurveCache<u32> = CurveCache::new(curve, Extent1d::from_min_max(i.a, i.b), opts);
    let mut o: Vec<f64> = Vec::new();
    match i.op {
        OpIn::Type => {
            o.extend([
                0.0,
                cc.type_code() as f64,
                b(cc.is_kind_of(14100)),
                b(cc.is_kind_of(solidcore::geom::object::OBJECT_TYPE)),
                b(cc.is_kind_of(14001)),
            ]);
        }
        OpIn::Analytic => {
            let is_analytic = cc.curve().is_analytic();
            match cc.analytic_curve(&Kernel::default()) {
                Ok(a) => {
                    o.push(0.0);
                    o.push(match a {
                        None => 0.0,
                        Some(_) if is_analytic => 1.0,
                        Some(_) => 2.0,
                    });
                }
                Err(e) => match Output::unported(&e) {
                    Some(d) => return Ok(d),
                    None => return Err(harness(e.to_string())),
                },
            }
        }
        OpIn::Tessellate => {
            if let Err(e) = cc.tessellate(&k) {
                if let Some(d) = Output::unported(&e) {
                    return Ok(d);
                }
                o.push(status(&e));
                return Ok(to_output(&o));
            }
            o.push(0.0);
            o.push(cc.span_count() as f64);
            for s in cc.spans() {
                put_aux(&mut o, &s.aux);
                for v in s.pseudo_box.basis() {
                    pv(&mut o, *v);
                }
                for ivl in s.pseudo_box.intervals() {
                    o.extend([ivl.min(), ivl.max()]);
                }
            }
            o.push(cc.continuities().len() as f64);
            o.extend(cc.continuities().iter().map(|c| f64::from(*c as i32)));
            match cc.tree() {
                Some(t) => tree_out(&mut o, t)?,
                None => o.push(0.0),
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
    let rational = rng.chance(0.2);
    let two_d = rng.chance(0.2);
    let a = if rng.chance(0.5) { 0.0 } else { rng.uniform(-5.0, 5.0) };
    let len = if rng.chance(0.5) { 1.0 } else { rng.uniform(0.1, 10.0) };
    let mut u = vec![a; p + 1];
    let inner = n - p;
    let mut ks: Vec<f64> = (0..inner).map(|_| a + len * rng.uniform(0.05, 0.95)).collect();
    ks.sort_by(f64::total_cmp);
    if inner >= 2 && p >= 2 && rng.chance(0.3) {
        // a repeated interior knot (multiplicity 2 <= p)
        if let Some(&v) = ks.first() {
            if let Some(x) = ks.get_mut(1) {
                *x = v;
            }
        }
    }
    u.extend(ks);
    u.extend(vec![a + len; p + 1]);
    let scale = if rng.chance(0.5) { 5.0 } else { 100.0 };
    let closed = rng.chance(0.08);
    let flat = rng.chance(0.08);
    let mut pts: Vec<[f64; 3]> = (0..=n)
        .map(|i| {
            let [x, y, z] = rp(rng, scale);
            [x + i as f64 * scale * 0.2, y, z]
        })
        .collect();
    if flat {
        // collinear control points (a line, possibly with uneven speed)
        let d = rp(rng, 1.0);
        for (i, q) in pts.iter_mut().enumerate() {
            let t = i as f64 + if rng.chance(0.5) { 0.0 } else { rng.uniform(-0.3, 0.3) };
            *q = [d[0] * t * scale, d[1] * t * scale, d[2] * t * scale];
        }
    }
    if closed && n >= 2 {
        if let Some(&first) = pts.first() {
            if let Some(last) = pts.last_mut() {
                *last = first;
            }
        }
    }
    let cpts = pts
        .into_iter()
        .map(|[x, y, z]| {
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
    BSplineIn { dim, cpts, p, u }
}

fn gen_hermite(rng: &mut Rng) -> HermiteIn {
    let s = if rng.chance(0.5) { 3.0 } else { 50.0 };
    let mut d1 = rp(rng, s);
    if rng.chance(0.1) {
        d1 = [0.0; 3];
    }
    let p1 = rp(rng, s);
    let mut p2 = rp(rng, s);
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
    if rng.chance(0.1) {
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
    if rng.chance(0.7) {
        CurveIn::Bspline(gen_bspline(rng))
    } else {
        CurveIn::Hermite(gen_hermite(rng))
    }
}

fn range_and_size(c: &CurveIn) -> (f64, f64, f64) {
    match c {
        CurveIn::Bspline(bs) => {
            let size = bs
                .cpts
                .iter()
                .map(|p| {
                    let w = if p[3] == NOW { 1.0 } else { p[3] };
                    let z = if p[2] == NOZ { 0.0 } else { p[2] / w };
                    (p[0] / w).abs().max((p[1] / w).abs()).max(z.abs())
                })
                .fold(1.0, f64::max);
            (
                bs.u.first().copied().unwrap_or(0.0),
                bs.u.last().copied().unwrap_or(1.0),
                size,
            )
        }
        CurveIn::Hermite(h) => {
            let size = [h.p1, h.d1, h.p2, h.d2]
                .iter()
                .flatten()
                .fold(1.0, |m: f64, v| m.max(v.abs()));
            (0.0, 1.0, size)
        }
        CurveIn::Circle(ci) => (ci.a0, ci.a1, ci.radius),
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let curve = gen_curve(rng);
    let (lo, hi, size) = range_and_size(&curve);
    let (a, bb) = match rng.below(6) {
        0 => {
            let x = lo + (hi - lo) * rng.uniform(-0.2, 1.2);
            let y = lo + (hi - lo) * rng.uniform(-0.2, 1.2);
            (x, y)
        }
        1 => {
            // inside, through interior knots
            let x = lo + (hi - lo) * rng.uniform(0.0, 0.5);
            (x, x + (hi - lo) * rng.uniform(0.1, 0.5))
        }
        2 => (lo, lo + (hi - lo) * rng.uniform(0.3, 1.0)),
        _ => (lo, hi),
    };
    let op = match rng.below(40) {
        0 => OpIn::Analytic,
        1 => OpIn::Type,
        _ => OpIn::Tessellate,
    };
    let mut o = OptsIn {
        full: rng.chance(0.3),
        aux: rng.chance(0.6),
        ..OptsIn::default()
    };
    match rng.below(4) {
        0 => o.chord = size * rng.uniform(1e-3, 3e-2),
        1 => o.angle = rng.uniform(3.0, 60.0),
        _ => {
            o.chord = size * rng.uniform(1e-3, 1e-1);
            o.angle = rng.uniform(5.0, 90.0);
        }
    }
    if rng.chance(0.03) {
        o.chord = 0.0;
        o.angle = 0.0;
    }
    if rng.chance(0.2) {
        o.offaxis = size * rng.uniform(1e-3, 1e-1);
    }
    if rng.chance(0.3) {
        o.minseg = rng.below(9);
    }
    if rng.chance(0.2) {
        o.max3d = size * rng.uniform(0.05, 0.8);
    }
    o.ratio = match rng.below(4) {
        0 => 1e-3,
        1 => 1e-2,
        _ => 1e-4,
    };
    Inputs {
        curve,
        a,
        b: bb,
        opts: o,
        op,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.curve_cache"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let i = gen_case(rng);
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
