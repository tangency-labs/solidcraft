//! Runner for unit `curve.curve_p7`: the local solve of the curve/curve intersection
//! global solver (`SmCCIGlobalSolver::LocalSolve`, run by a whole `SolveIt` or on one
//! leaf pair) and `SmCurve::GlobalPropertyAnalysis` (with its check comb).
//!
//! C++: `solidcore_ffi::units::curve_curve_p7` (upstream `source/SMLib/src/SmCurve.cpp`;
//! the solver class is a verbatim copy in the shim, the property analysis is called through
//! `SmCurve*`). Rust: `solidcore::curve::curve::p7`.
//!
//! A case is one `op` with its curves (`{"bspline": {dim, oob, cpts, p, u}}` with control
//! points as stored `[x, y, z, w]`, `{"hermite": {dim, p1, d1, p2, d2}}` or
//! `{"circle": ..}`, a quadratic B-spline arc, as in unit `curve.curve_p3`), the curve
//! caches (`null`: the curve's object cache; else the `SmCurveCache` arguments) and the
//! solver parameters. Both sides produce the flat record documented in the ffi module;
//! the runner reports its first entry as the `status` int (the first cache build status:
//! `0` = `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a
//! Rust error upstream cannot produce) and the rest as the `out` values. Solution nodes
//! are compared as pre-order indices in their trees.
//!
//! `{"o", "x", "y", "scale"?, "repeat"?}` transforms (`property` `xf`) are applied on both sides
//! (`SmAxis2Placement::SetCanonical` + `SmCurve::Transform` / `Curve::transform`) before
//! the op; a circle with `quintic` is `SM_CO_QUINTIC`.
//!
//! Deferred cases (unit `curve.curve_p5`): the leaf pairs that reach a Newton
//! intersection (`LocalCurveIntersect`, through `SmCCIntersectENFO::Evaluate`) and the
//! property solves that reach `SmCurve::LocalPropertyAnalysis` (every span the property
//! solver does not cull).
//!
//! Generated circle-derived curvature-extrema cases use noncircular ellipses: a
//! circle's constant curvature otherwise makes the number of noise-selected extrema
//! machine-dependent. The original flat-circle case remains a full native regression
//! outside the canonical report corpus; no solver or comparison rule is changed.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::{CciGlobalSolver, Curve, CurvePropertyType};
use solidcore::curve::curve::p5::make_curve_cache;
use solidcore::curve::curve::p7::PropertyAnalysisOptions;
use solidcore::curve::curve_cache::{CurveCache, CurveCacheOptions};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::global_solver::{
    Branch, GlobalSolve, SolutionRequested, SolveRequest, SolverOperation,
};
use solidcore::geom::solution_array::{ObjectKind, Solution, SolutionObject};
use solidcore::geom::tree::{CurveSpanRef, NodeData, NodeId, Tree};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, ProjectionType};
use solidcore::nurbs::math::NOW;
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::SolidError;
use solidcore_ffi::units::curve_curve_p7::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `curve.curve_p7` runner.
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
    /// `SM_CO_QUINTIC` (degree 5) instead of `SM_CO_QUADRATIC`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    quintic: bool,
}

/// A transform applied to a curve before the op, `repeat` times
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

/// The curve/curve intersection solver set-up.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CciIn {
    c1: CurveIn,
    /// `None`: self intersection (both trees are the first curve's).
    c2: Option<CurveIn>,
    cache1: Option<CacheIn>,
    cache2: Option<CacheIn>,
    ivl1: [f64; 2],
    ivl2: [f64; 2],
    sop: i32,
    tol: f64,
    #[serde(default)]
    proj: bool,
    #[serde(default)]
    ptype: i32,
    #[serde(default)]
    vecs: Vec<[f64; 3]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    CciSolve {
        #[serde(flatten)]
        cci: CciIn,
    },
    CciLocal {
        #[serde(flatten)]
        cci: CciIn,
        leaf1: u64,
        leaf2: u64,
    },
    Property {
        curve: CurveIn,
        /// Transforms applied to `curve` first.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        xf: Vec<XformIn>,
        ivl: [f64; 2],
        prop: i32,
        val: f64,
        vecs: Vec<[f64; 3]>,
        tol: f64,
        comb: bool,
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
        if ci.quintic {
            solidcore::nurbs::b_spline_curve::NurbCircleParam::Quintic
        } else {
            solidcore::nurbs::b_spline_curve::NurbCircleParam::Quadratic
        },
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

fn cache_block(v: &mut Vec<f64>, cache: Option<&CacheIn>) {
    match cache {
        None => v.push(0.0),
        Some(c) => {
            v.push(1.0);
            v.extend_from_slice(&c.ivl);
            v.extend([b(c.full), c.chord, c.angle, c.offaxis]);
            v.extend([c.minseg as f64, b(c.aux), c.max3d, c.ratio]);
        }
    }
}

fn cpp_input(op: &OpIn) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut v = Vec::new();
    let code = match op {
        OpIn::CciSolve { cci } | OpIn::CciLocal { cci, .. } => {
            curve_block(&mut v, &cci.c1)?;
            v.push(b(cci.c2.is_none()));
            if let Some(c2) = &cci.c2 {
                curve_block(&mut v, c2)?;
            }
            cache_block(&mut v, cci.cache1.as_ref());
            if cci.c2.is_some() {
                cache_block(&mut v, cci.cache2.as_ref());
            }
            v.extend_from_slice(&cci.ivl1);
            v.extend_from_slice(&cci.ivl2);
            v.extend([f64::from(cci.sop), cci.tol, b(cci.proj), f64::from(cci.ptype)]);
            push_vecs(&mut v, &cci.vecs);
            if let OpIn::CciLocal { leaf1, leaf2, .. } = op {
                v.extend([*leaf1 as f64, *leaf2 as f64]);
                Op::CciLocal
            } else {
                Op::CciSolve
            }
        }
        OpIn::Property {
            curve,
            xf,
            ivl,
            prop,
            val,
            vecs,
            tol,
            comb,
        } => {
            curve_block(&mut v, curve)?;
            push_xf(&mut v, xf);
            v.extend_from_slice(ivl);
            v.extend([f64::from(*prop), 1.0, *val]);
            push_vecs(&mut v, vecs);
            v.extend([*tol, b(*comb)]);
            Op::Property
        }
    };
    Ok((code, v))
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
        CurveIn::Circle(_) => Err(harness("unresolved circle")),
    }
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

/// A Rust failure: deferred for `Unported`, else the status.
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

#[derive(Debug, Clone, Copy, PartialEq)]
struct Obj(i64, Option<Extent1d>);

/// A handle of curve `c` (upstream's `SmCurve*`: a curve object with the curve's
/// natural interval, read by `SmSolution::IsEqual`).
fn curve_obj(h: i64, c: &Curve) -> Obj {
    Obj(h, Some(c.natural_interval()))
}

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

/// Pre-order index of every node of `tree` and its span (leaf data) nodes in that order.
fn index_nodes(tree: &Tree<Obj>) -> Result<(BTreeMap<NodeId, usize>, Vec<NodeId>), HarnessError> {
    let mut idx = BTreeMap::new();
    let mut leaves = Vec::new();
    let mut stack: Vec<NodeId> = tree.top_node().into_iter().collect();
    while let Some(id) = stack.pop() {
        let nd = tree.node(id).map_err(|e| harness(e.to_string()))?;
        let k = idx.len();
        idx.insert(id, k);
        if matches!(nd.data, NodeData::Curve(CurveSpanRef::Span(_))) {
            leaves.push(id);
        }
        if let Some(c) = nd.child2 {
            stack.push(c);
        }
        if let Some(c) = nd.child1 {
            stack.push(c);
        }
    }
    Ok((idx, leaves))
}

/// A solution record (see the ffi module docs).
fn put_sol(
    o: &mut Vec<f64>,
    s: &Solution<Obj>,
    n1: &BTreeMap<NodeId, usize>,
    n2: &BTreeMap<NodeId, usize>,
) {
    let id = |x: Option<Obj>| x.map_or(-1.0, |v| v.0 as f64);
    let nd = |m: &BTreeMap<NodeId, usize>, x: Option<NodeId>| {
        x.map_or(-1.0, |n| m.get(&n).map_or(-2.0, |&i| i as f64))
    };
    let np = s.num_variables.clamp(2, 10);
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
        nd(n1, s.nodes[0]),
        // only for two objects (see the shim)
        if s.num_objects > 1 {
            nd(n2, s.nodes[1])
        } else {
            -1.0
        },
        s.start.solution_value,
    ]);
    o.extend(s.start.parameters.iter().take(np));
    o.push(s.end.solution_value);
    o.extend(s.end.parameters.iter().take(np));
}

fn ivl_of(i: [f64; 2]) -> Extent1d {
    Extent1d::from_min_max(i[0], i[1])
}

/// Builds the cache of `c` (handle `h`); `Err` carries the status record entry.
fn build_cache(
    k: &Kernel,
    c: &Curve,
    cache: Option<&CacheIn>,
    h: i64,
) -> Result<CurveCache<Obj>, Fail> {
    match cache {
        None => make_curve_cache::<Obj>(k, c, Some(curve_obj(h, c))).map_err(|e| {
            if e.is_unported() {
                Fail::from(e)
            } else {
                // GetOrCreateObjectCache returns NULL
                Fail::Status(1006.0)
            }
        }),
        Some(ci) => {
            let opts = CurveCacheOptions {
                make_full_tree: ci.full,
                chord_height_tolerance: ci.chord,
                angle_tol_deg: ci.angle,
                off_axis_tolerance: ci.offaxis,
                min_segments: ci.minseg,
                compute_aux_tree_data: ci.aux,
                max_3d_distance: ci.max3d,
                min_parametric_ratio: ci.ratio,
            };
            let mut cc: CurveCache<Obj> = CurveCache::new(c.clone(), ivl_of(ci.ivl), opts);
            cc.set_curve_handle(Some(curve_obj(h, c)));
            cc.tessellate(k).map_err(Fail::from)?;
            if cc.tree().is_none() {
                return Err(Fail::Status(1006.0));
            }
            Ok(cc)
        }
    }
}

fn projection_type(code: i32) -> Result<ProjectionType, HarnessError> {
    match code {
        0 => Ok(ProjectionType::Parallel),
        1 => Ok(ProjectionType::Perspective),
        2 => Ok(ProjectionType::Rotation),
        _ => Err(harness("projection type")),
    }
}

#[allow(clippy::too_many_lines)]
fn rust_cci(cci: &CciIn, leaves: Option<(u64, u64)>) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut o: Vec<f64> = Vec::new();
    let c1 = build(&cci.c1)?;
    let c2 = match &cci.c2 {
        Some(c) => Some(build(c)?),
        None => None,
    };
    macro_rules! cache_or_fail {
        ($c:expr, $cache:expr, $h:expr) => {
            match build_cache(&k, $c, $cache, $h) {
                Ok(cc) => {
                    o.push(0.0);
                    Some(cc)
                }
                Err(Fail::Deferred(d)) => return Ok(*d),
                Err(Fail::Status(s)) => {
                    o.push(s);
                    None
                }
            }
        };
    }
    let k1 = cache_or_fail!(&c1, cci.cache1.as_ref(), 0);
    let k2 = match &c2 {
        Some(c) => Some(cache_or_fail!(c, cci.cache2.as_ref(), 1)),
        None => None,
    };
    let t1 = k1.as_ref().and_then(CurveCache::tree);
    let t2 = match &k2 {
        Some(k) => k.as_ref().and_then(CurveCache::tree),
        None => t1,
    };
    let (Some(t1), Some(t2)) = (t1, t2) else {
        return Ok(to_output(&o));
    };
    let curve2 = c2.as_ref().unwrap_or(&c1);
    let (n1, l1) = index_nodes(t1)?;
    let (n2, l2) = index_nodes(t2)?;
    let sop = SolverOperation::from_code(cci.sop).ok_or_else(|| harness("operation"))?;
    let vecs: Vec<Vec3> = cci.vecs.iter().map(|&x| v3(x)).collect();
    let mut gs = CciGlobalSolver::new(
        t1,
        t2,
        ivl_of(cci.ivl1),
        ivl_of(cci.ivl2),
        cci.proj,
        projection_type(cci.ptype)?,
    )
    .with_curves(&c1, curve2, &k);
    match leaves {
        None => {
            let mut sols = solidcore::geom::solution_array::SolutionArray::new();
            let req = SolveRequest {
                operation: sop,
                solution_requested: SolutionRequested::All,
                tolerance_3d: cci.tol,
                best_answer_so_far_sq: solidcore::math::BIG_DOUBLE,
                opt_vectors: &vecs,
            };
            match gs.solve_it(&req, &mut sols) {
                Ok(()) => {
                    o.extend([0.0, sols.len() as f64]);
                    for s in sols.as_slice() {
                        put_sol(&mut o, s, &n1, &n2);
                    }
                }
                Err(e) => match Fail::from(e) {
                    Fail::Deferred(d) => return Ok(*d),
                    Fail::Status(s) => o.push(s),
                },
            }
        }
        Some((leaf1, leaf2)) => {
            o.extend([l1.len() as f64, l2.len() as f64]);
            let (Some(&a), Some(&bb)) = (
                l1.get((leaf1 as usize) % l1.len().max(1)),
                l2.get((leaf2 as usize) % l2.len().max(1)),
            ) else {
                return Ok(to_output(&o));
            };
            gs.base.operation = sop;
            gs.base.tolerance_3d = cci.tol;
            gs.base.best_answer_so_far_sq = solidcore::math::BIG_DOUBLE;
            gs.base.opt_vectors = vecs;
            let branch: Branch = [Some(a), Some(bb), None];
            match gs.local_solve(&branch) {
                Ok(needs) => {
                    let sols = gs.base.solutions.as_slice();
                    o.extend([0.0, b(needs), sols.len() as f64]);
                    for s in sols {
                        put_sol(&mut o, s, &n1, &n2);
                    }
                }
                Err(e) => match Fail::from(e) {
                    Fail::Deferred(d) => return Ok(*d),
                    Fail::Status(s) => o.push(s),
                },
            }
        }
    }
    Ok(to_output(&o))
}

fn rust(op: &OpIn) -> Result<Output, HarnessError> {
    match op {
        OpIn::CciSolve { cci } => rust_cci(cci, None),
        OpIn::CciLocal { cci, leaf1, leaf2 } => rust_cci(cci, Some((*leaf1, *leaf2))),
        OpIn::Property {
            curve,
            xf,
            ivl,
            prop,
            val,
            vecs,
            tol,
            comb,
        } => {
            let k = Kernel::default();
            let mut c = build(curve)?;
            apply_xf(&k, &mut c, xf)?;
            let prop = CurvePropertyType::from_code(*prop).ok_or_else(|| harness("property"))?;
            let vs: Vec<Vec3> = vecs.iter().map(|&x| v3(x)).collect();
            let opts = PropertyAnalysisOptions {
                property_value: Some(*val),
                opt_vectors: &vs,
                want_comb: *comb,
                handle: Some(curve_obj(0, &c)),
            };
            let mut o: Vec<f64> = Vec::new();
            match c.global_property_analysis(&k, &ivl_of(*ivl), prop, *tol, &opts) {
                Ok(r) => {
                    // the solutions name nodes of the curve's (deterministic) cache tree
                    let cache = make_curve_cache::<Obj>(&k, &c, Some(curve_obj(0, &c)))
                        .map_err(|e| harness(e.to_string()))?;
                    let tree = cache.tree().ok_or_else(|| harness("no tree"))?;
                    let (n1, _) = index_nodes(tree)?;
                    o.extend([0.0, r.solutions.len() as f64]);
                    for s in r.solutions.as_slice() {
                        put_sol(&mut o, s, &n1, &n1);
                    }
                    o.push(r.comb_points.len() as f64);
                    for p in &r.comb_points {
                        o.extend([p.x, p.y, p.z]);
                    }
                    o.push(r.comb_vecs.len() as f64);
                    for p in &r.comb_vecs {
                        o.extend([p.x, p.y, p.z]);
                    }
                }
                Err(e) => match Fail::from(e) {
                    Fail::Deferred(d) => return Ok(*d),
                    Fail::Status(s) => o.push(s),
                },
            }
            Ok(to_output(&o))
        }
    }
}

// ---- generation -------------------------------------------------------------------

fn rp(rng: &mut Rng, s: f64) -> [f64; 3] {
    rng.point3(-s, s)
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
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

/// A unit vector.
fn gen_unit(rng: &mut Rng) -> [f64; 3] {
    loop {
        let v = rp(rng, 1.0);
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if l > 0.2 {
            return scale3(v, 1.0 / l);
        }
    }
}

/// A random clamped B-spline (sometimes rational or with repeated interior knots).
fn gen_bspline(rng: &mut Rng, scale: f64) -> BSplineIn {
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(5) as usize;
    let rational = rng.chance(0.2);
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
    let cpts = (0..=n)
        .map(|i| {
            let [x, y, z] = rp(rng, scale);
            let x = x + i as f64 * scale * 0.4;
            if rational {
                let w = rng.uniform(0.3, 3.0);
                [x * w, y * w, z * w, w]
            } else {
                [x, y, z, NOW]
            }
        })
        .collect();
    BSplineIn {
        dim: 3,
        oob: false,
        cpts,
        p,
        u,
    }
}

fn gen_hermite(rng: &mut Rng, s: f64) -> HermiteIn {
    let p1 = rp(rng, s);
    let mut p2 = rp(rng, s);
    if rng.chance(0.05) {
        p2 = p1;
    }
    HermiteIn {
        dim: 3,
        p1,
        d1: rp(rng, s),
        p2,
        d2: rp(rng, s),
    }
}

fn gen_circle(rng: &mut Rng, closed: bool) -> CurveIn {
    let a0 = rng.uniform(-90.0, 90.0);
    let c = CurveIn::Circle(CircleIn {
        center: rp(rng, 3.0),
        x: [1.0, 0.0, 0.0],
        y: [0.0, rng.uniform(0.5, 1.0), rng.uniform(-0.5, 0.5)],
        radius: rng.uniform(0.5, 5.0),
        a0,
        a1: if closed {
            a0 + 360.0
        } else {
            a0 + rng.uniform(30.0, 330.0)
        },
        quintic: false,
    });
    resolve(&c).unwrap_or(c)
}

fn gen_curve(rng: &mut Rng) -> CurveIn {
    gen_curve_with_circle_origin(rng).0
}

fn gen_curve_with_circle_origin(rng: &mut Rng) -> (CurveIn, bool) {
    let s = if rng.chance(0.7) { 3.0 } else { 30.0 };
    match rng.below(10) {
        0 => (gen_circle(rng, false), true),
        1 => (gen_circle(rng, true), true),
        2..=6 => (CurveIn::Bspline(gen_bspline(rng, s)), false),
        _ => (CurveIn::Hermite(gen_hermite(rng, s)), false),
    }
}

// Generated circles have their X axis along global X. Stretching homogeneous X
// by exactly two gives a noncircular rational ellipse without new random draws.
fn stretch_circle_x(curve: &mut CurveIn) {
    if let CurveIn::Bspline(bs) = curve {
        for point in &mut bs.cpts {
            point[0] *= 2.0;
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
    }
}

fn point_at(k: &Kernel, c: &CurveIn, t: f64) -> [f64; 3] {
    build(c)
        .ok()
        .and_then(|cv| cv.evaluate_point(k, t).ok())
        .map_or([0.0; 3], |p| [p.x, p.y, p.z])
}

fn tangent_at(k: &Kernel, c: &CurveIn, t: f64) -> [f64; 3] {
    build(c)
        .ok()
        .and_then(|cv| cv.evaluate(k, t, 1, true, true).ok())
        .and_then(|v| v.get(1).copied())
        .map_or([1.0, 0.0, 0.0], |p| [p.x, p.y, p.z])
}

/// An increasing sub-interval of `c` (often the whole range).
fn gen_ivl(rng: &mut Rng, c: &CurveIn) -> [f64; 2] {
    let (a, bb) = range_of(c);
    if rng.chance(0.6) {
        return [a, bb];
    }
    let x = a + (bb - a) * rng.uniform(-0.1, 0.9);
    let y = x + (bb - a) * rng.uniform(0.05, 0.8);
    [x.max(a - (bb - a)), y]
}

/// A second curve related to `c1`: crossing it at a curve point, touching it, a copy
/// (coincident), a nearby copy, or unrelated.
fn gen_partner(k: &Kernel, rng: &mut Rng, c1: &CurveIn) -> CurveIn {
    let (a, bb) = range_of(c1);
    let t = a + (bb - a) * rng.unit();
    let p = point_at(k, c1, t);
    let tan = tangent_at(k, c1, t);
    match rng.below(10) {
        // a segment crossing the curve at p (sometimes ending exactly on it)
        0..=3 => {
            let d = if rng.chance(0.2) {
                // nearly tangent
                add(scale3(tan, 0.3), scale3(gen_unit(rng), 0.02))
            } else {
                scale3(gen_unit(rng), rng.uniform(0.5, 3.0))
            };
            let (s, e) = if rng.chance(0.3) {
                (p, add(p, d))
            } else {
                (sub(p, d), add(p, d))
            };
            let bend = rp(rng, 0.5);
            CurveIn::Hermite(HermiteIn {
                dim: 3,
                p1: s,
                d1: add(scale3(sub(e, s), 1.0), bend),
                p2: e,
                d2: sub(e, s),
            })
        }
        // a degree 1-2 B-spline through p
        4 | 5 => {
            let d = scale3(gen_unit(rng), rng.uniform(0.5, 3.0));
            let mid = add(p, rp(rng, 0.3));
            let deg = 1 + rng.below(2) as usize;
            let cpts = if deg == 1 {
                vec![
                    [p[0] - d[0], p[1] - d[1], p[2] - d[2], NOW],
                    [p[0] + d[0], p[1] + d[1], p[2] + d[2], NOW],
                ]
            } else {
                vec![
                    [p[0] - d[0], p[1] - d[1], p[2] - d[2], NOW],
                    [mid[0], mid[1], mid[2], NOW],
                    [p[0] + d[0], p[1] + d[1], p[2] + d[2], NOW],
                ]
            };
            let u0 = rng.uniform(-2.0, 2.0);
            let mut u = vec![u0; deg + 1];
            u.extend(vec![u0 + rng.uniform(0.5, 3.0); deg + 1]);
            CurveIn::Bspline(BSplineIn {
                dim: 3,
                oob: false,
                cpts,
                p: deg,
                u,
            })
        }
        // the same curve (coincident)
        6 => c1.clone(),
        // a slightly offset copy
        7 => {
            let off = scale3(gen_unit(rng), *rng.choose(&[1e-9, 1e-5, 1e-2]).unwrap_or(&1e-5));
            match resolve(c1).unwrap_or_else(|_| c1.clone()) {
                CurveIn::Bspline(mut bs) => {
                    for c in &mut bs.cpts {
                        let w = if c[3] == NOW { 1.0 } else { c[3] };
                        c[0] += off[0] * w;
                        c[1] += off[1] * w;
                        c[2] += off[2] * w;
                    }
                    CurveIn::Bspline(bs)
                }
                CurveIn::Hermite(mut h) => {
                    h.p1 = add(h.p1, off);
                    h.p2 = add(h.p2, off);
                    CurveIn::Hermite(h)
                }
                other @ CurveIn::Circle(_) => other,
            }
        }
        // a curve starting at a point of the first one
        8 => {
            let e = add(p, scale3(cross(tan, gen_unit(rng)), 1.0));
            CurveIn::Hermite(HermiteIn {
                dim: 3,
                p1: p,
                d1: sub(e, p),
                p2: e,
                d2: rp(rng, 1.0),
            })
        }
        _ => gen_curve(rng),
    }
}

fn gen_cache(rng: &mut Rng, c: &CurveIn) -> Option<CacheIn> {
    if rng.chance(0.6) {
        return None;
    }
    let (a, bb) = range_of(c);
    let mut cache = CacheIn {
        ivl: [a, bb],
        full: rng.chance(0.5),
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
        aux: rng.chance(0.6),
        max3d: 0.0,
        ratio: 1e-4,
    };
    if cache.chord == 0.0 && cache.angle == 0.0 {
        cache.angle = 20.0;
    }
    Some(cache)
}

/// A self-intersecting (looping) curve.
fn gen_loop(rng: &mut Rng) -> CurveIn {
    let s = rng.uniform(1.0, 5.0);
    let z = if rng.chance(0.5) { 0.0 } else { 1e-3 };
    let cpts = vec![
        [0.0, 0.0, 0.0, NOW],
        [2.0 * s, s, z, NOW],
        [0.0, 2.0 * s, -z, NOW],
        [-s, s, z, NOW],
        [s, -0.5 * s, 0.0, NOW],
        [3.0 * s, 0.2 * s, z, NOW],
    ];
    let p = 2 + rng.below(2) as usize;
    let n = cpts.len() - 1;
    let mut u = vec![0.0; p + 1];
    for i in 1..(n + 1 - p) {
        u.push(i as f64 / (n + 1 - p) as f64);
    }
    u.extend(vec![1.0; p + 1]);
    CurveIn::Bspline(BSplineIn {
        dim: 3,
        oob: false,
        cpts,
        p,
        u,
    })
}

fn gen_cci(k: &Kernel, rng: &mut Rng) -> CciIn {
    let self_int = rng.chance(0.15);
    let c1 = if self_int && rng.chance(0.6) {
        gen_loop(rng)
    } else {
        gen_curve(rng)
    };
    let c2 = if self_int {
        None
    } else {
        Some(gen_partner(k, rng, &c1))
    };
    let cache1 = gen_cache(rng, &c1);
    let cache2 = c2.as_ref().and_then(|c| gen_cache(rng, c));
    let ivl1 = gen_ivl(rng, &c1);
    let ivl2 = gen_ivl(rng, c2.as_ref().unwrap_or(&c1));
    let (sop, proj, ptype, vecs) = match rng.below(10) {
        0..=4 => (0, false, 0, Vec::new()),
        5 | 6 => (2, false, 0, Vec::new()),
        7 => (16, true, 0, vec![gen_unit(rng)]),
        8 => {
            let axis = gen_unit(rng);
            let x = cross(axis, gen_unit(rng));
            let l = (x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt();
            let x = if l > 1e-3 {
                scale3(x, 1.0 / l)
            } else {
                [1.0, 0.0, 0.0]
            };
            (17, true, 2, vec![rp(rng, 2.0), axis, x])
        }
        _ => {
            let n = gen_unit(rng);
            let eye = add(scale3(n, rng.uniform(10.0, 50.0)), rp(rng, 2.0));
            (18, true, 1, vec![rp(rng, 1.0), n, eye])
        }
    };
    let (sop, proj, ptype, vecs) = if [16, 17, 18].contains(&sop) && vecs.len() != if sop == 16 { 1 } else { 3 } {
        (0, false, 0, Vec::new())
    } else {
        (sop, proj, ptype, vecs)
    };
    CciIn {
        c1,
        c2,
        cache1,
        cache2,
        ivl1,
        ivl2,
        sop,
        tol: *rng.choose(&[1e-6, 1e-6, 1e-3, 1e-2, 0.1]).unwrap_or(&1e-6),
        proj,
        ptype,
        vecs,
    }
}

fn gen_property(k: &Kernel, rng: &mut Rng) -> OpIn {
    let (mut curve, from_circle) = gen_curve_with_circle_origin(rng);
    let prop = rng.below(29) as i32;
    if from_circle && matches!(prop, 13 | 14) {
        stretch_circle_x(&mut curve);
    }
    let (a, bb) = range_of(&curve);
    let t = a + (bb - a) * rng.unit();
    let p = point_at(k, &curve, t);
    let (val, vecs) = match prop {
        // planes: through a curve point, or far away (no span reaches it)
        15 | 16 => {
            let n = gen_unit(rng);
            let q = if rng.chance(0.5) {
                p
            } else {
                add(p, scale3(n, rng.uniform(50.0, 500.0)))
            };
            let d = -(n[0] * q[0] + n[1] * q[1] + n[2] * q[2]);
            (d, vec![n, gen_unit(rng), gen_unit(rng)])
        }
        _ => {
            let o = if rng.chance(0.5) { p } else { rp(rng, 10.0) };
            (rng.uniform(0.0, 5.0), vec![o, gen_unit(rng), gen_unit(rng)])
        }
    };
    OpIn::Property {
        ivl: gen_ivl(rng, &curve),
        curve,
        xf: Vec::new(),
        prop,
        val,
        vecs,
        tol: *rng.choose(&[1e-6, 1e-3, 0.1]).unwrap_or(&1e-6),
        comb: rng.chance(0.5),
    }
}

/// A leaf pair (indices into the pre-order span lists) whose tolerance-expanded node
/// boxes overlap, so the local solve gets past its box tests; `None` when the caches
/// cannot be built or no pair overlaps.
fn overlapping_leaves(rng: &mut Rng, cci: &CciIn) -> Option<(u64, u64)> {
    let k = Kernel::default();
    let c1 = build(&cci.c1).ok()?;
    let c2 = match &cci.c2 {
        Some(c) => Some(build(c).ok()?),
        None => None,
    };
    let k1 = build_cache(&k, &c1, cci.cache1.as_ref(), 0).ok()?;
    let k2 = match &c2 {
        Some(c) => Some(build_cache(&k, c, cci.cache2.as_ref(), 1).ok()?),
        None => None,
    };
    let t1 = k1.tree()?;
    let t2 = k2.as_ref().map_or(Some(t1), CurveCache::tree)?;
    let (_, l1) = index_nodes(t1).ok()?;
    let (_, l2) = index_nodes(t2).ok()?;
    let mut pairs = Vec::new();
    for (i, &a) in l1.iter().enumerate() {
        let mut ba = t1.node(a).ok()?.bbox;
        ba.expand_absolute(cci.tol);
        for (j, &bb) in l2.iter().enumerate() {
            if !ba.are_disjoint(&t2.node(bb).ok()?.bbox, 0.0) {
                pairs.push((i as u64, j as u64));
            }
        }
    }
    rng.choose(&pairs).copied()
}

fn gen_case(k: &Kernel, rng: &mut Rng) -> OpIn {
    match rng.below(10) {
        0..=2 => OpIn::CciSolve { cci: gen_cci(k, rng) },
        3..=7 => {
            let cci = gen_cci(k, rng);
            let (leaf1, leaf2) = if rng.chance(0.8) {
                overlapping_leaves(rng, &cci).unwrap_or((rng.below(64), rng.below(64)))
            } else {
                (rng.below(64), rng.below(64))
            };
            OpIn::CciLocal { cci, leaf1, leaf2 }
        }
        _ => gen_property(k, rng),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.curve_p7"
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

    // Compare the complete ordered native result, including its solution count.
    // Constant-curvature circles intentionally have no frozen extrema count.
    fn assert_native_parity(op: &OpIn) -> (usize, f64) {
        let native = cpp(op).unwrap();
        let port = rust(op).unwrap();
        for out in [&native, &port] {
            assert!(out.error.is_none(), "{out:?}");
            assert!(out.deferred.is_none(), "{out:?}");
            assert!(out.rejected.is_none(), "{out:?}");
            assert_eq!(out.ints.get("status"), Some(&vec![0]));
            assert!(!out.values["out"].is_empty());
        }
        let comparison = crate::compare(&native, &port, &[]);
        assert!(comparison.pass(), "{comparison:?}");
        assert!(comparison.values_compared > 0);
        (comparison.values_compared, native.values["out"][0])
    }

    #[test]
    fn flat_circle_original_gen1409_retains_full_native_parity() {
        let op: OpIn = serde_json::from_str(include_str!("../../tests/data/curve_p7_flat_circle.json")).unwrap();
        assert!(assert_native_parity(&op).1 > 0.0);
    }

    #[test]
    fn generated_ellipse_extrema_have_stable_report_shapes() {
        // Frozen after replacing the seven circle-derived extrema inputs. CI on
        // Linux must reproduce these macOS shapes AND compare every native value.
        // No case may become rejected, deferred or error-only.
        for (index, prop, expected_values) in [
            (191, 14, 942), (510, 14, 981), (1409, 13, 929),
            (1526, 13, 16), (1682, 13, 955), (2276, 14, 916), (2526, 14, 16),
        ] {
            let seed = CaseContext::seed_for("curve.curve_p7", index);
            let input = Runner.generate(&mut Rng::new(seed)).unwrap();
            let op: OpIn = serde_json::from_value(input).unwrap();
            assert!(matches!(&op, OpIn::Property { prop: actual, .. } if *actual == prop));
            assert_eq!(assert_native_parity(&op).0, expected_values, "gen{index:04}, seed {seed}");
        }
    }

    #[test]
    fn stretched_circle_has_known_nonconstant_curvature_and_native_extrema() {
        // Semiaxes 4 and 2: rho_min = b*b/a = 1, rho_max = a*a/b = 8.
        let circle = CurveIn::Circle(CircleIn {
            center: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0],
            radius: 2.0, a0: 0.0, a1: 360.0, quintic: false,
        });
        let mut ellipse = resolve(&circle).unwrap();
        stretch_circle_x(&mut ellipse);
        let curve = build(&ellipse).unwrap();
        let (a, b) = range_of(&ellipse);
        for (fraction, expected) in [(0.0, 1.0), (0.25, 8.0), (0.5, 1.0), (0.75, 8.0)] {
            let derivatives = curve.evaluate(&Kernel::default(), a + fraction * (b - a), 2, true, false).unwrap();
            let speed = derivatives[1].length();
            let radius = speed * speed * speed / derivatives[1].cross(derivatives[2]).length();
            assert!((radius - expected).abs() <= 1e-9 * expected, "{radius} != {expected}");
        }
        for prop in [13, 14] {
            let (_, solutions) = assert_native_parity(&OpIn::Property {
                curve: ellipse.clone(), xf: Vec::new(), ivl: [a, b], prop, val: 0.0,
                vecs: vec![[0.0; 3]; 3], tol: 1e-6, comb: true,
            });
            assert!(solutions > 0.0);
        }
    }

    /// Generated cases cover every op, with passing (not deferred) cases.
    #[test]
    fn generated_cases_are_diverse() {
        let k = &Kernel::default();
        let mut rng = Rng::new(7);
        let mut stats: BTreeMap<String, [usize; 4]> = BTreeMap::new();
        for _ in 0..600 {
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
        assert_eq!(stats.len(), 3);
    }
}
