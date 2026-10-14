//! Runner for unit `intersect.surface_silhouette`: `SmSurfaceSilhouette`.
//!
//! C++: `solidcore_ffi::units::intersect_surface_silhouette` (upstream
//! `source/SMLib/src/SmSurfaceSilhouette.cpp` on a real `SmBSplineSurface` or `SmPlane`).
//! Rust: `solidcore::intersect::surface_silhouette::SurfaceSilhouette`.
//!
//! A case is a surface, a tracer domain, a view direction or eye point, optional tolerances and
//! a program of ops: `ComputePointValues` (alone or with the previous point, which reaches the
//! singularity searches through `SmFindSilSingEFO`), `LocalPointSolve`, `DoesPointLieOnCurve`,
//! `FindSolutionsOnBoundaryCurve` and `BranchMayContainAnswers` on hand-built surface cache
//! trees. Both sides produce the record documented in the ffi module, reported as the values
//! `rec`.
//!
//! Deferred cases: `SmSurface::EvaluateNormal` on B-spline surfaces and `SmSurface::DropVectors`
//! (`surface.surface_p7`), `SmSurface::EvaluateGeometric` and `SmSurface::EvaluateNormalSection`
//! (`surface.surface_p3`), and whatever the boundary property analysis and the polar box
//! builds still defer. A case is deferred as a whole, so programs are short.
//!
//! At least [`MIN_GENERATED`] generated cases run; generation only draws inputs the ffi
//! wrapper accepts.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::global_solver::{Branch, GlobalSolver};
use solidcore::geom::polar_box::PolarBox;
use solidcore::geom::pseudo_box::PseudoBox;
use solidcore::geom::solution_array::Solution;
use solidcore::geom::tree::{
    AuxDataType, NodeClass, NodeData, NodeGeomType, NodeId, SplitDir, SurfaceAux, Tree, TreeNode,
    TreeOwner,
};
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::surface_silhouette::SurfaceSilhouette;
use solidcore::intersect::surface_tracer::{BoundaryCurve, TracePoint, TracePointType};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::surface::surface_cache::p1::{
    BezierAux2d, BezierPatch, SurfaceCache, SurfaceCacheOptions,
};
use solidcore::SolidError;
use solidcore_ffi::units::intersect_surface_silhouette::{self as ffi};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// `NL_NOW`: the weight of a non-rational control point.
const NOW: f64 = -f64::MAX;

/// `intersect.surface_silhouette` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -------------------------------------------------------------------------

/// A B-spline surface (`w` = [`NOW`] when non-rational).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SurfData {
    p: usize,
    q: usize,
    net: Vec<Vec<[f64; 4]>>,
    u: Vec<f64>,
    v: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SurfIn {
    Bspline(SurfData),
    Plane {
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: [f64; 2],
        domain: [f64; 4],
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CurveData {
    p: usize,
    cpts: Vec<[f64; 4]>,
    knots: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NodeIn {
    parent: Option<usize>,
    #[serde(default)]
    c1: Option<usize>,
    #[serde(default)]
    c2: Option<usize>,
    aux: u8,
    #[serde(default)]
    null_data: bool,
    dom: [f64; 4],
    #[serde(default)]
    polar: Option<Vec<[f64; 3]>>,
    #[serde(default)]
    pseudo: Option<PseudoIn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PseudoIn {
    basis: [[f64; 3]; 3],
    pts: Vec<[f64; 3]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Cpv {
        #[serde(default = "one")]
        dir: f64,
        #[serde(default)]
        use_prev: bool,
        #[serde(default = "normal_kind")]
        kind: u8,
        #[serde(default)]
        t: [f64; 3],
        uv: [f64; 2],
    },
    Lps {
        uv: [f64; 2],
    },
    OnCurve {
        refine: bool,
        uv: [f64; 2],
    },
    Boundary {
        curve: CurveData,
        interval: [f64; 2],
    },
    Branch {
        nodes: Vec<NodeIn>,
        queries: Vec<usize>,
    },
}

fn one() -> f64 {
    1.0
}

fn normal_kind() -> u8 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    surface: SurfIn,
    domain: [f64; 4],
    perspective: bool,
    eye: [f64; 3],
    #[serde(default)]
    tol: Option<f64>,
    #[serde(default)]
    ang: Option<f64>,
    ops: Vec<OpIn>,
}

// ---- C++ side -----------------------------------------------------------------------

fn ffi_surface(s: &SurfIn) -> ffi::SurfaceIn {
    match s {
        SurfIn::Bspline(d) => ffi::SurfaceIn::BSpline(ffi::SurfaceData {
            p: d.p,
            q: d.q,
            net: d.net.clone(),
            u: d.u.clone(),
            v: d.v.clone(),
        }),
        SurfIn::Plane {
            o,
            x,
            y,
            scale,
            domain,
        } => ffi::SurfaceIn::Plane {
            o: *o,
            x: *x,
            y: *y,
            scale: *scale,
            domain: *domain,
        },
    }
}

fn ffi_curve(c: &CurveData) -> ffi::CurveData {
    ffi::CurveData {
        p: c.p,
        cpts: c.cpts.clone(),
        knots: c.knots.clone(),
    }
}

fn ffi_node(n: &NodeIn) -> ffi::NodeIn {
    ffi::NodeIn {
        parent: n.parent,
        child1: n.c1,
        child2: n.c2,
        aux: n.aux,
        null_data: n.null_data,
        domain: n.dom,
        polar: n.polar.clone(),
        pseudo: n.pseudo.as_ref().map(|p| ffi::PseudoIn {
            basis: p.basis,
            points: p.pts.clone(),
        }),
    }
}

fn ffi_program(i: &Inputs) -> ffi::Program {
    let ops = i
        .ops
        .iter()
        .map(|o| match o {
            OpIn::Cpv {
                dir,
                use_prev,
                kind,
                t,
                uv,
            } => ffi::Op::Cpv {
                dir: *dir,
                use_prev: *use_prev,
                kind: *kind,
                t: *t,
                uv: *uv,
            },
            OpIn::Lps { uv } => ffi::Op::Lps(*uv),
            OpIn::OnCurve { refine, uv } => ffi::Op::OnCurve {
                refine: *refine,
                uv: *uv,
            },
            OpIn::Boundary { curve, interval } => ffi::Op::Boundary {
                curve: ffi_curve(curve),
                interval: *interval,
            },
            OpIn::Branch { nodes, queries } => ffi::Op::Branch {
                nodes: nodes.iter().map(ffi_node).collect(),
                queries: queries.clone(),
            },
        })
        .collect();
    ffi::Program {
        surface: ffi_surface(&i.surface),
        domain: i.domain,
        perspective: i.perspective,
        eye: i.eye,
        tol: i.tol,
        ang: i.ang,
        ops,
    }
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    match ffi::run(&ffi_program(i)) {
        Ok(rec) => Ok(Output::new().values("rec", rec)),
        Err(e) => Err(harness(format!("C++: {e}"))),
    }
}

// ---- Rust side ----------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn p2(p: [f64; 2]) -> Point2 {
    Vec2::new(p[0], p[1])
}

fn ext(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

fn b(x: bool) -> f64 {
    if x {
        1.0
    } else {
        0.0
    }
}

fn cpoint(c: &[f64; 4]) -> CPoint {
    CPoint::new(c[0], c[1], c[2], c[3])
}

fn rust_surface(s: &SurfIn) -> Result<Surface, HarnessError> {
    match s {
        SurfIn::Plane {
            o,
            x,
            y,
            scale,
            domain,
        } => Ok(Surface::from(Plane::new(
            v3(*o),
            v3(*x),
            v3(*y),
            Vec2::new(scale[0], scale[1]),
            &ext(*domain),
        ))),
        SurfIn::Bspline(d) => {
            let net = d
                .net
                .iter()
                .map(|r| r.iter().map(cpoint).collect())
                .collect();
            let sur = NurbSurface::new(net, d.p, d.q, d.u.clone(), d.v.clone())
                .map_err(|e| harness(format!("surface: {e}")))?;
            Ok(Surface::from(BSplineSurface::from_nurb(sur, false)))
        }
    }
}

fn rust_curve(d: &CurveData) -> Result<BSplineCurve, HarnessError> {
    let pw: Vec<CPoint> = d.cpts.iter().map(cpoint).collect();
    let n = pw
        .len()
        .checked_sub(1)
        .ok_or_else(|| harness("empty curve"))?;
    let m = d
        .knots
        .len()
        .checked_sub(1)
        .ok_or_else(|| harness("no knots"))?;
    BSplineCurve::from_data(3, n, m, d.p, &pw, &d.knots)
        .map_err(|e| harness(format!("B-spline: {e}")))
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

/// A failed Rust operation: the whole case is deferred, or the harness failed.
enum Fail {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Fail::Harness(e)
    }
}

/// `Ok(Ok(v))`, `Ok(Err(status))`, or a deferral.
fn st<T>(r: solidcore::Result<T>) -> Result<Result<T, f64>, Fail> {
    match r {
        Ok(v) => Ok(Ok(v)),
        Err(e) => {
            if let Some(d) = Output::unported(&e) {
                return Err(Fail::Deferred(Box::new(d)));
            }
            Ok(Err(status(&e)))
        }
    }
}

fn put_vec(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_uv(o: &mut Vec<f64>, v: Vec2) {
    o.extend([v.x, v.y]);
}

/// The shim's `put_point`.
fn put_point(o: &mut Vec<f64>, p: &TracePoint) {
    o.push(f64::from(p.point_type as i32));
    put_vec(o, p.curve_3d[0]);
    put_vec(o, p.curve_3d[1]);
    put_uv(o, p.curve_uv[0]);
    put_uv(o, p.curve_uv[1]);
    let s = &p.surface;
    for v in [s[0][0], s[1][0], s[0][1], s[1][1], s[2][0], s[0][2], s[2][2]] {
        put_vec(o, v);
    }
}

/// The shim's `put_sol`.
fn put_sol(o: &mut Vec<f64>, s: &Solution<BoundaryCurve>) {
    let np = s.num_variables.clamp(2, 10);
    o.extend([
        f64::from(s.solution_type as i32),
        s.num_variables as f64,
        s.num_objects as f64,
        b(s.objects[0].is_some()),
        s.start.solution_value,
    ]);
    o.extend(s.start.parameters.iter().take(np));
    o.push(s.end.solution_value);
    o.extend(s.end.parameters.iter().take(np));
}

fn kind_of(code: u8) -> Result<TracePointType, HarnessError> {
    TracePointType::from_code(i32::from(code)).ok_or_else(|| harness("point type"))
}

/// The shim's `read_tree` in a cache of `surface` over its natural domain.
fn build_cache(
    surface: &Surface,
    nodes: &[NodeIn],
) -> Result<SurfaceCache<BoundaryCurve>, HarnessError> {
    let handle = BoundaryCurve::default();
    let mut cache = SurfaceCache::new(
        surface.clone(),
        surface.natural_uv_domain(),
        SurfaceCacheOptions::default(),
    );
    cache.set_surface_handle(Some(handle));
    let mut tree: Tree<BoundaryCurve> = Tree::new(Some(TreeOwner::SurfaceCache));
    let mut patches = Vec::new();
    let mut auxs = Vec::new();
    for (k, n) in nodes.iter().enumerate() {
        let aux_type = AuxDataType::from_code(i32::from(n.aux))
            .ok_or_else(|| harness(format!("bad aux type {}", n.aux)))?;
        let mut aux = BezierAux2d {
            uv_domain: ext(n.dom),
            owning_tree_node: Some(NodeId(k)),
            ..BezierAux2d::default()
        };
        if let Some(vs) = &n.polar {
            let mut pb = PolarBox::new();
            for &v in vs {
                pb.add_vector(v3(v), None);
            }
            aux.polar_box = pb;
        }
        let slot = if aux_type == AuxDataType::BezierSurface {
            let mut pseudo = PseudoBox::new();
            if let Some(ps) = &n.pseudo {
                let [b1, b2, b3] = ps.basis;
                // upstream ignores the status (the basis stays as it was)
                let _ = pseudo.set_basis(v3(b1), v3(b2), v3(b3));
                for &p in &ps.pts {
                    pseudo.add_point(v3(p));
                }
            }
            patches.push(BezierPatch {
                aux,
                pseudo_box: pseudo,
                ..BezierPatch::default()
            });
            patches.len() - 1
        } else {
            auxs.push(aux);
            auxs.len() - 1
        };
        tree.add_node(TreeNode {
            parent: n.parent.map(NodeId),
            child1: n.c1.map(NodeId),
            child2: n.c2.map(NodeId),
            aux_type,
            data: NodeData::Surface(SurfaceAux {
                split_dir: SplitDir::Unknown,
                is_marked: false,
                node_class: NodeClass::Unknown,
                face: None,
                surface: Some(handle),
                slot: (!n.null_data).then_some(slot),
            }),
            geom_type: NodeGeomType::Default,
            ..TreeNode::new()
        });
    }
    tree.set_top_node(Some(NodeId(0)));
    *cache.patches_mut() = patches;
    *cache.auxs_mut() = auxs;
    cache.set_tree(Some(tree));
    Ok(cache)
}

fn run_op(
    sil: &mut SurfaceSilhouette<'_>,
    k: &Kernel,
    surface: &Surface,
    op: &OpIn,
    last: &mut Option<TracePoint>,
    o: &mut Vec<f64>,
) -> Result<(), Fail> {
    match op {
        OpIn::Cpv {
            dir,
            use_prev,
            kind,
            t,
            uv,
        } => {
            let mut p = TracePoint::new();
            p.point_type = kind_of(*kind)?;
            p.curve_3d[1] = v3(*t);
            sil.base.curve_trace_direction = *dir;
            let prev = if *use_prev { *last } else { None };
            match st(sil.compute_point_values(p2(*uv), &mut p, prev.as_ref()))? {
                Ok(()) => {
                    o.push(0.0);
                    put_point(o, &p);
                    *last = Some(p);
                }
                Err(s) => o.push(s),
            }
        }
        OpIn::Lps { uv } => match st(sil.local_point_solve(k, &TracePoint::new(), p2(*uv)))? {
            Ok(r) => {
                o.push(0.0);
                o.push(b(r.is_some()));
                if let Some(found) = r {
                    put_uv(o, found);
                }
            }
            Err(s) => o.push(s),
        },
        OpIn::OnCurve { refine, uv } => {
            let mut q = p2(*uv);
            match st(sil.does_point_lie_on_curve(k, &mut q, *refine))? {
                Ok(f) => {
                    o.push(0.0);
                    o.push(b(f));
                    put_uv(o, q);
                }
                Err(s) => o.push(s),
            }
        }
        OpIn::Boundary { curve, interval } => {
            let c = rust_curve(curve)?;
            let ivl = Extent1d::from_min_max(interval[0], interval[1]);
            match st(sil.find_solutions_on_boundary_curve(k, &c, &ivl))? {
                Ok(sols) => {
                    o.push(0.0);
                    o.push(sols.as_slice().len() as f64);
                    for s in sols.as_slice() {
                        put_sol(o, s);
                    }
                }
                Err(s) => o.push(s),
            }
        }
        OpIn::Branch { nodes, queries } => {
            let cache = build_cache(surface, nodes)?;
            let tree = cache.tree().ok_or_else(|| harness("no tree"))?;
            let solver = GlobalSolver::with_tree(tree, 2);
            o.push(0.0);
            for &q in queries {
                let mut branch: Branch = Default::default();
                if let Some(slot) = branch.first_mut() {
                    *slot = Some(NodeId(q));
                }
                match st(sil.branch_may_contain_answers(&solver, &cache, &branch))? {
                    Ok(f) => o.push(b(f)),
                    Err(s) => o.push(s),
                }
            }
        }
    }
    Ok(())
}

fn op_code(op: &OpIn) -> f64 {
    match op {
        OpIn::Cpv { .. } => 1.0,
        OpIn::Lps { .. } => 2.0,
        OpIn::OnCurve { .. } => 3.0,
        OpIn::Boundary { .. } => 4.0,
        OpIn::Branch { .. } => 5.0,
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let surface = rust_surface(&i.surface)?;
    let mut sil = match SurfaceSilhouette::new(&surface, &ext(i.domain), i.perspective, v3(i.eye))
    {
        Ok(s) => s,
        Err(e) => {
            return Output::unported(&e)
                .ok_or_else(|| harness(format!("silhouette construction: {e}")));
        }
    };
    let mut o = vec![sil.base.approx_tol_3d, sil.base.ang_tol_rad];
    if let Some(tol) = i.tol {
        sil.base.approx_tol_3d = tol;
    }
    if let Some(ang) = i.ang {
        sil.base.ang_tol_rad = ang;
    }
    let mut last = None;
    for op in &i.ops {
        o.push(op_code(op));
        match run_op(&mut sil, &k, &surface, op, &mut last, &mut o) {
            Ok(()) => {}
            Err(Fail::Deferred(d)) => return Ok(*d),
            Err(Fail::Harness(h)) => return Err(h),
        }
    }
    o.push(sil.base.curve_trace_direction);
    Ok(Output::new().values("rec", o))
}

// ---- generation ---------------------------------------------------------------------

fn pick<T: Copy>(rng: &mut Rng, xs: &[T], default: T) -> T {
    rng.choose(xs).copied().unwrap_or(default)
}

fn unit_dir(rng: &mut Rng) -> [f64; 3] {
    loop {
        let p = rng.point3(-1.0, 1.0);
        let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        if l > 0.1 && l <= 1.0 {
            return [p[0] / l, p[1] / l, p[2] / l];
        }
    }
}

/// A clamped knot vector on `[a, b]` for `n1` control points of degree `p`.
fn gen_knots(rng: &mut Rng, n1: usize, p: usize, a: f64, b: f64) -> Vec<f64> {
    let interior = n1 - p - 1;
    let mut inner: Vec<f64> = if rng.chance(0.5) {
        (1..=interior)
            .map(|i| a + (b - a) * i as f64 / (interior + 1) as f64)
            .collect()
    } else {
        (0..interior)
            .map(|_| a + (b - a) * rng.uniform(0.1, 0.9))
            .collect()
    };
    inner.sort_by(f64::total_cmp);
    let mut k = vec![a; p + 1];
    k.extend(inner);
    k.extend(vec![b; p + 1]);
    k
}

/// A random B-spline surface over roughly `[0, scale]^2`.
fn gen_random_bspline(rng: &mut Rng, scale: f64) -> SurfData {
    let n1 = 2 + rng.below(5) as usize;
    let m1 = 2 + rng.below(5) as usize;
    let p = 1 + rng.below(3.min(n1 as u64 - 1)) as usize;
    let q = 1 + rng.below(3.min(m1 as u64 - 1)) as usize;
    let rational = rng.chance(0.2);
    let amp = pick(rng, &[0.0, 0.2, 0.5, 1.0, 2.0], 0.5);
    let (a, c) = (rng.uniform(-3.0, 3.0), rng.uniform(-3.0, 3.0));
    let (u0, u1) = (a, a + rng.uniform(0.5, 6.0));
    let (v0, v1) = (c, c + rng.uniform(0.5, 6.0));
    let jitter = rng.uniform(0.0, 0.2);
    let net = (0..n1)
        .map(|i| {
            (0..m1)
                .map(|j| {
                    let x = i as f64 / (n1 - 1) as f64 + rng.uniform(-jitter, jitter) * 0.2;
                    let y = j as f64 / (m1 - 1) as f64 + rng.uniform(-jitter, jitter) * 0.2;
                    let z = amp * rng.uniform(-1.0, 1.0);
                    let w = if rational { rng.uniform(0.5, 2.0) } else { NOW };
                    [x * scale, y * scale, z * scale, w]
                })
                .collect()
        })
        .collect();
    SurfData {
        p,
        q,
        net,
        u: gen_knots(rng, n1, p, u0, u1),
        v: gen_knots(rng, m1, q, v0, v1),
    }
}

/// A surface given by a height function `z(x, y)` sampled on a `n1 x m1` net of degree
/// `p x q` over `[-1, 1]^2`, times `scale`.
fn height_surface(n1: usize, m1: usize, p: usize, q: usize, scale: f64, z: impl Fn(f64, f64) -> f64) -> SurfData {
    let net = (0..n1)
        .map(|i| {
            (0..m1)
                .map(|j| {
                    let x = -1.0 + 2.0 * i as f64 / (n1 - 1) as f64;
                    let y = -1.0 + 2.0 * j as f64 / (m1 - 1) as f64;
                    [x * scale, y * scale, z(x, y) * scale, NOW]
                })
                .collect()
        })
        .collect();
    let knots = |n1: usize, p: usize| {
        let interior = n1 - p - 1;
        let mut k = vec![0.0; p + 1];
        k.extend((1..=interior).map(|i| i as f64 / (interior + 1) as f64));
        k.extend(vec![1.0; p + 1]);
        k
    };
    SurfData {
        p,
        q,
        net,
        u: knots(n1, p),
        v: knots(m1, q),
    }
}

/// A quarter cylinder of radius `scale` about the y axis (rational quadratic in u).
fn quarter_cylinder(scale: f64) -> SurfData {
    let w = core::f64::consts::FRAC_1_SQRT_2;
    let row = |x: f64, z: f64, wt: f64| -> Vec<[f64; 4]> {
        (0..3)
            .map(|j| {
                let y = -1.0 + f64::from(j);
                [x * scale, y * scale, z * scale, wt]
            })
            .collect()
    };
    SurfData {
        p: 2,
        q: 2,
        net: vec![row(1.0, 0.0, 1.0), row(1.0, 1.0, w), row(0.0, 1.0, 1.0)],
        u: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        v: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    }
}

fn gen_bspline(rng: &mut Rng, scale: f64) -> SurfData {
    match rng.below(8) {
        // dome: elliptic points everywhere
        0 => height_surface(4, 4, 2, 2, scale, |x, y| 1.0 - 0.5 * (x * x + y * y)),
        // saddle: hyperbolic points
        1 => height_surface(3, 3, 2, 2, scale, |x, y| 0.5 * (x * x - y * y)),
        // parabolic cylinder: zero Gaussian curvature
        2 => height_surface(4, 3, 2, 2, scale, |x, _| 0.5 * x * x),
        // monkey-saddle-like wave: curvature sign changes
        3 => height_surface(6, 6, 3, 3, scale, |x, y| 0.3 * (x * x * x - 3.0 * x * y * y)),
        4 => quarter_cylinder(scale),
        _ => gen_random_bspline(rng, scale),
    }
}

fn gen_surface(rng: &mut Rng, scale: f64) -> SurfIn {
    if rng.chance(0.15) {
        let x = unit_dir(rng);
        let mut y = unit_dir(rng);
        let dot = x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
        if dot.abs() > 0.9 {
            y = [x[1], -x[0], 0.0];
            if (y[0] * y[0] + y[1] * y[1]) < 1e-6 {
                y = [0.0, x[2], -x[1]];
            }
        }
        let o = rng.point3(-scale, scale);
        let s = [rng.uniform(0.5, 2.0), rng.uniform(0.5, 2.0)];
        let (u0, v0) = (
            rng.uniform(-2.0, 2.0) * scale,
            rng.uniform(-2.0, 2.0) * scale,
        );
        SurfIn::Plane {
            o,
            x,
            y,
            scale: s,
            domain: [
                u0,
                v0,
                u0 + rng.uniform(0.5, 4.0) * scale,
                v0 + rng.uniform(0.5, 4.0) * scale,
            ],
        }
    } else {
        SurfIn::Bspline(gen_bspline(rng, scale))
    }
}

fn natural(s: &SurfIn) -> [f64; 4] {
    match s {
        SurfIn::Plane { domain, .. } => *domain,
        SurfIn::Bspline(d) => [
            d.u.first().copied().unwrap_or(0.0),
            d.v.first().copied().unwrap_or(0.0),
            d.u.last().copied().unwrap_or(1.0),
            d.v.last().copied().unwrap_or(1.0),
        ],
    }
}

fn gen_domain(rng: &mut Rng, nat: [f64; 4]) -> [f64; 4] {
    if rng.chance(0.6) {
        return nat;
    }
    let (du, dv) = (nat[2] - nat[0], nat[3] - nat[1]);
    let a = rng.uniform(0.0, 0.4);
    let bb = rng.uniform(0.6, 1.0);
    let c = rng.uniform(0.0, 0.4);
    let d = rng.uniform(0.6, 1.0);
    [
        nat[0] + a * du,
        nat[1] + c * dv,
        nat[0] + bb * du,
        nat[1] + d * dv,
    ]
}

/// A point of `d`, sometimes exactly on its boundary.
fn uv_in(rng: &mut Rng, d: [f64; 4]) -> [f64; 2] {
    let snap = |rng: &mut Rng, lo: f64, hi: f64| match rng.below(8) {
        0 => lo,
        1 => hi,
        _ => rng.uniform(lo, hi),
    };
    [snap(rng, d[0], d[2]), snap(rng, d[1], d[3])]
}

/// A guess for `LocalPointSolve`: usually in the domain, sometimes outside (clamped).
fn guess_in(rng: &mut Rng, d: [f64; 4]) -> [f64; 2] {
    if rng.chance(0.15) {
        let (du, dv) = (d[2] - d[0], d[3] - d[1]);
        [
            d[0] + du * rng.uniform(-0.5, 1.5),
            d[1] + dv * rng.uniform(-0.5, 1.5),
        ]
    } else {
        uv_in(rng, d)
    }
}

/// The view: a direction (parallel) or an eye point (perspective).
fn gen_view(rng: &mut Rng, scale: f64) -> (bool, [f64; 3]) {
    if rng.chance(0.4) {
        let d = unit_dir(rng);
        let r = scale * pick(rng, &[3.0, 10.0, 1.5, 100.0], 3.0);
        let mut e = [d[0] * r, d[1] * r, d[2] * r];
        if rng.chance(0.15) {
            // an eye on the far side
            e[2] = -e[2].abs() - scale;
        }
        (true, e)
    } else {
        let d = match rng.below(10) {
            0 => [0.0, 0.0, 1.0],
            1 => [1.0, 0.0, 0.0],
            2 => [0.0, 1.0, 0.0],
            3 => [0.0, 0.0, 0.0],
            4 => [0.0, 0.0, -5.0],
            5 => [1e-12, 0.0, 0.0],
            _ => unit_dir(rng),
        };
        (false, d)
    }
}

/// A boundary iso curve of a B-spline surface (a row or column of its net with that
/// direction's knots), or a random curve.
fn gen_boundary(rng: &mut Rng, s: &SurfIn, scale: f64) -> OpIn {
    let curve = match s {
        SurfIn::Bspline(d) if rng.chance(0.8) => {
            let m1 = d.net.first().map_or(0, Vec::len);
            match rng.below(4) {
                0 => CurveData {
                    p: d.q,
                    cpts: d.net.first().cloned().unwrap_or_default(),
                    knots: d.v.clone(),
                },
                1 => CurveData {
                    p: d.q,
                    cpts: d.net.last().cloned().unwrap_or_default(),
                    knots: d.v.clone(),
                },
                2 => CurveData {
                    p: d.p,
                    cpts: d.net.iter().filter_map(|r| r.first().copied()).collect(),
                    knots: d.u.clone(),
                },
                _ => CurveData {
                    p: d.p,
                    cpts: d
                        .net
                        .iter()
                        .filter_map(|r| r.get(m1.saturating_sub(1)).copied())
                        .collect(),
                    knots: d.u.clone(),
                },
            }
        }
        _ => {
            let n1 = 2 + rng.below(5) as usize;
            let p = 1 + rng.below(3.min(n1 as u64 - 1)) as usize;
            let cpts = (0..n1)
                .map(|_| {
                    let q = rng.point3(-scale, scale);
                    [q[0], q[1], q[2], NOW]
                })
                .collect();
            let end = rng.uniform(0.5, 3.0);
            CurveData {
                p,
                cpts,
                knots: gen_knots(rng, n1, p, 0.0, end),
            }
        }
    };
    let (k0, km) = (
        curve.knots.first().copied().unwrap_or(0.0),
        curve.knots.last().copied().unwrap_or(1.0),
    );
    let interval = if rng.chance(0.7) {
        [k0, km]
    } else {
        let a = k0 + (km - k0) * rng.uniform(0.0, 0.4);
        let bb = k0 + (km - k0) * rng.uniform(0.6, 1.0);
        [a, bb]
    };
    OpIn::Boundary { curve, interval }
}

/// A small tree over `nat`: a root and up to three generations of halved children.
fn gen_tree(rng: &mut Rng, nat: [f64; 4], scale: f64) -> OpIn {
    let mut nodes = vec![NodeIn {
        parent: None,
        c1: None,
        c2: None,
        aux: if rng.chance(0.5) { 4 } else { 3 },
        null_data: false,
        dom: nat,
        polar: None,
        pseudo: None,
    }];
    let depth = rng.below(4) as usize;
    let mut frontier = vec![0usize];
    for _ in 0..depth {
        let mut next = Vec::new();
        for &pi in &frontier {
            let Some(pd) = nodes.get(pi).map(|n| n.dom) else {
                continue;
            };
            if nodes.len() + 2 > ffi::MAX_NODES {
                break;
            }
            let split_u = rng.chance(0.5);
            let f = pick(rng, &[0.5, 0.5, 0.3, 0.7], 0.5);
            let (d1, d2) = if split_u {
                let m = pd[0] + (pd[2] - pd[0]) * f;
                ([pd[0], pd[1], m, pd[3]], [m, pd[1], pd[2], pd[3]])
            } else {
                let m = pd[1] + (pd[3] - pd[1]) * f;
                ([pd[0], pd[1], pd[2], m], [pd[0], m, pd[2], pd[3]])
            };
            let k1 = nodes.len();
            for d in [d1, d2] {
                nodes.push(NodeIn {
                    parent: Some(pi),
                    c1: None,
                    c2: None,
                    aux: 3,
                    null_data: false,
                    dom: d,
                    polar: None,
                    pseudo: None,
                });
            }
            if let Some(p) = nodes.get_mut(pi) {
                p.c1 = Some(k1);
                p.c2 = Some(k1 + 1);
                if p.aux == 3 && pi != 0 {
                    p.aux = 4;
                }
            }
            next.extend([k1, k1 + 1]);
        }
        frontier = next;
    }
    let n = nodes.len();
    for k in 0..n {
        let is_parent = nodes.iter().any(|o| o.parent == Some(k));
        let Some(nd) = nodes.get_mut(k) else {
            continue;
        };
        if rng.chance(0.08) {
            nd.aux = pick(rng, &[0, 2, 6, 7], 6);
        }
        if !is_parent && rng.chance(0.08) {
            nd.null_data = true;
        }
        if rng.chance(0.4) {
            let base = unit_dir(rng);
            let spread = pick(rng, &[0.0, 0.05, 0.3, 1.0, 2.0], 0.3);
            let cnt = 1 + rng.below(5) as usize;
            nd.polar = Some(
                (0..cnt)
                    .map(|_| {
                        let j = rng.point3(-spread, spread);
                        [base[0] + j[0], base[1] + j[1], base[2] + j[2]]
                    })
                    .collect(),
            );
        }
        if nd.aux == 3 && rng.chance(0.7) {
            let basis = if rng.chance(0.5) {
                [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
            } else {
                let a = unit_dir(rng);
                let c = unit_dir(rng);
                [a, c, unit_dir(rng)]
            };
            let cnt = 1 + rng.below(8) as usize;
            let pts = (0..cnt).map(|_| rng.point3(-scale, scale)).collect();
            nd.pseudo = Some(PseudoIn { basis, pts });
        }
    }
    let nq = 1 + rng.below(n.min(8) as u64) as usize;
    let queries = (0..nq).map(|_| rng.below(n as u64) as usize).collect();
    OpIn::Branch { nodes, queries }
}

/// A `ComputePointValues` chain along a segment of the domain.
fn gen_chain(rng: &mut Rng, domain: [f64; 4], ops: &mut Vec<OpIn>) {
    let a = uv_in(rng, domain);
    let bb = uv_in(rng, domain);
    let steps = 1 + rng.below(5) as usize;
    let dir = if rng.chance(0.8) { 1.0 } else { -1.0 };
    let kind = match rng.below(10) {
        0 => 0,
        1 => 2,
        _ => 1,
    };
    let t = if kind == 2 && rng.chance(0.5) {
        unit_dir(rng)
    } else {
        [0.0, 0.0, 0.0]
    };
    for s in 0..=steps {
        let f = s as f64 / steps as f64;
        let mut uv = [a[0] + (bb[0] - a[0]) * f, a[1] + (bb[1] - a[1]) * f];
        // stay inside the closed domain despite rounding
        uv[0] = uv[0].clamp(domain[0], domain[2]);
        uv[1] = uv[1].clamp(domain[1], domain[3]);
        ops.push(OpIn::Cpv {
            dir,
            use_prev: s > 0 && rng.chance(0.85),
            kind,
            t,
            uv,
        });
    }
}

fn gen_inputs(rng: &mut Rng) -> Inputs {
    let scale = pick(rng, &[1.0, 1.0, 1.0, 1.0, 1e-3, 1e3, 1e-6, 1e5], 1.0);
    let surface = gen_surface(rng, scale);
    let nat = natural(&surface);
    let domain = gen_domain(rng, nat);
    let (perspective, eye) = gen_view(rng, scale);
    let tol = if rng.chance(0.25) {
        Some(pick(rng, &[1e-3, 1e-5, 1e-2, 0.0], 1e-3) * scale)
    } else {
        None
    };
    let ang = if rng.chance(0.2) {
        Some(pick(rng, &[0.1, 0.5, 1e-3, 1.5], 0.1))
    } else {
        None
    };
    let mut ops = Vec::new();
    match rng.below(10) {
        0..=3 => gen_chain(rng, domain, &mut ops),
        4 | 5 => {
            for _ in 0..=rng.below(3) {
                ops.push(OpIn::Lps {
                    uv: guess_in(rng, domain),
                });
            }
        }
        6 => {
            for _ in 0..=rng.below(3) {
                ops.push(OpIn::OnCurve {
                    refine: rng.chance(0.4),
                    uv: uv_in(rng, nat),
                });
            }
        }
        7 => ops.push(gen_boundary(rng, &surface, scale)),
        8 => ops.push(gen_tree(rng, nat, scale)),
        _ => {
            gen_chain(rng, domain, &mut ops);
            ops.push(OpIn::Lps {
                uv: guess_in(rng, domain),
            });
        }
    }
    Inputs {
        surface,
        domain,
        perspective,
        eye,
        tol,
        ang,
        ops,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.surface_silhouette"
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

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_inputs(rng)).ok()
    }
}
