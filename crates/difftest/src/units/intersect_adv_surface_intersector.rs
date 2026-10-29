//! Runner for unit `intersect.adv_surface_intersector`: `SmAdvSurfaceIntersector`.
//!
//! C++: `solidcore_ffi::units::intersect_adv_surface_intersector` (a real
//! `SmAdvSurfaceIntersector` on `SmPlane` / `SmBSplineSurface` objects with hand-built
//! decomposition trees in their surface caches). Rust:
//! `solidcore::intersect::adv_surface_intersector`.
//!
//! A case is two surfaces (B may be A itself), their domains, the fillet flag, optional
//! trees (one per surface cache), a point arena and a program of operations; both sides
//! produce the record documented in the ffi module (statuses `0` = `SM_SUCCESS`, else
//! the `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a Rust error upstream
//! cannot return, [`ffi::REJECTED`] for an op upstream would crash on), reported as the
//! values `rec`. A case whose Rust side reaches an unported unit is deferred as a whole.
//!
//! Three kinds of programs are generated: singular points (local surface geometry
//! built from a shape tensor per surface: planar, parabolic, elliptic, hyperbolic,
//! umbilic, near-threshold curvatures, coincident or sharing a principal curvature,
//! opposite normals, previous directions, curve points, boundary UV positions, tiny
//! and huge scales), Dupin indicatrices (including degenerate evaluations) and global
//! solves over trees sampled from the surfaces (`SolveIt` for `SM_SO_INTERSECT`,
//! `SM_SO_INTERSECTION_TEST` and `SM_SO_MINIMIZE`, `LocalSolve`, `SolveBranch`,
//! `FindBestSubdivisionNode`, `BranchIsGaussSeparate`, `FindInteriorCurves`).
//!
//! Deferred cases: those whose Rust side reaches `SmSurface::LocalSurfaceIntersect`
//! (`surface.surface_p11`), from `LocalSolve` of leaf boxes more than four times apart
//! in size or `FindInteriorCurves` with a candidate pair on a B-spline or on planes
//! more than 5 degrees apart.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent1d::{Extent1d, ExtentPointType};
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::global_solver::{
    GlobalSolve, SolutionRequested, SolveRequest, SolverOperation, MAX_SOLVER_TREES,
};
use solidcore::geom::polar_box::PolarBox;
use solidcore::geom::pseudo_box::PseudoBox;
use solidcore::geom::solution_array::{ObjectKind, SolutionArray, SolutionObject};
use solidcore::geom::tree::{
    AuxDataType, NodeData, NodeGeomType, NodeId, SurfaceAux, Tree, TreeNode, TreeOwner,
};
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::adv_surface_intersector::{AdvSurfaceIntersector, InteriorCurves};
use solidcore::intersect::surface_intersector::p1::{
    IntersectionPointType, SrfIndex, TsectId, TsectPnt,
};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::surface::surface_cache::p1::{
    BezierAux2d, BezierPatch, SurfaceCache, SurfaceCacheOptions,
};
use solidcore::SolidError;
use solidcore_ffi::units::intersect_adv_surface_intersector::{self as ffi};
use solidcore_ffi::units::intersect_surface_intersector_p1 as p1ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, CaseSource, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `intersect.adv_surface_intersector` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// `SM_BIG_DOUBLE`.
const BIG: f64 = 1.0e20;

/// The surface handle type of the caches (`SmSurface*`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct H(i64);

impl SolutionObject for H {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Surface
    }
}

// ---- inputs ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
enum SurfaceIn {
    Plane {
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: [f64; 2],
        domain: [f64; 4],
    },
    BSpline {
        p: usize,
        q: usize,
        /// Rows (u index) of `[x, y, z, w]` (homogeneous); `w` = `null` when non-rational.
        net: Vec<Vec<[Option<f64>; 4]>>,
        u: Vec<f64>,
        v: Vec<f64>,
    },
}

/// Basis vectors and `[min, max]` intervals of a pseudo box.
type PseudoIn = ([[f64; 3]; 3], [[f64; 2]; 3]);

/// A named singular point configuration: surface matrices and previous tangent.
type Config = (&'static str, [[V; 3]; 3], [[V; 3]; 3], Option<V>);

/// An evaluation matrix `[i][j]`.
type EvalIn = [[[f64; 3]; 3]; 3];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NodeIn {
    parent: i64,
    c1: i64,
    c2: i64,
    aux: u8,
    bbox: [f64; 6],
    dom: [f64; 4],
    polar: Vec<[f64; 3]>,
    /// Basis vectors and `[min, max]` intervals (patches only).
    pseudo: Option<PseudoIn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
enum OpIn {
    State,
    Set {
        dir: f64,
        ang: f64,
        approx: f64,
        tol: f64,
        sop: u8,
    },
    Push {
        i: usize,
    },
    Sing {
        i: usize,
        prev: Option<usize>,
    },
    Dupin {
        eval: Box<EvalIn>,
        normal: [f64; 3],
    },
    Solve {
        sop: u8,
        req: u8,
        tol: f64,
        best: f64,
    },
    BestSub {
        a: usize,
        b: usize,
    },
    Gauss {
        a: usize,
        b: usize,
    },
    Local {
        a: usize,
        b: usize,
    },
    Interior,
    SolveBranch {
        a: usize,
        b: usize,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    a: SurfaceIn,
    /// `None`: B is A itself.
    b: Option<SurfaceIn>,
    dom1: [f64; 4],
    dom2: [f64; 4],
    fillet: bool,
    /// Tree of A and (unless B is A) of B.
    trees: Option<(Vec<NodeIn>, Option<Vec<NodeIn>>)>,
    /// [`p1ffi::POINT_LEN`] values each.
    points: Vec<Vec<f64>>,
    ops: Vec<OpIn>,
}

fn cpt4(c: &[Option<f64>; 4]) -> [f64; 4] {
    [
        c[0].unwrap_or(f64::NAN),
        c[1].unwrap_or(f64::NAN),
        c[2].unwrap_or(f64::NAN),
        c[3].unwrap_or(NOW),
    ]
}

fn ffi_surface(s: &SurfaceIn) -> p1ffi::SurfaceIn {
    match s {
        SurfaceIn::Plane {
            o,
            x,
            y,
            scale,
            domain,
        } => p1ffi::SurfaceIn::Plane {
            o: *o,
            x: *x,
            y: *y,
            scale: *scale,
            domain: *domain,
        },
        SurfaceIn::BSpline { p, q, net, u, v } => p1ffi::SurfaceIn::BSpline {
            p: *p,
            q: *q,
            net: net.iter().map(|r| r.iter().map(cpt4).collect()).collect(),
            u: u.clone(),
            v: v.clone(),
        },
    }
}

fn ffi_node(n: &NodeIn) -> ffi::NodeIn {
    ffi::NodeIn {
        parent: n.parent,
        c1: n.c1,
        c2: n.c2,
        aux: n.aux,
        bbox: n.bbox,
        dom: n.dom,
        polar: n.polar.clone(),
        pseudo: n.pseudo,
    }
}

fn ffi_op(o: &OpIn) -> ffi::Op {
    match o {
        OpIn::State => ffi::Op::State,
        OpIn::Set {
            dir,
            ang,
            approx,
            tol,
            sop,
        } => ffi::Op::Set {
            dir: *dir,
            ang: *ang,
            approx: *approx,
            tol: *tol,
            op: *sop,
        },
        OpIn::Push { i } => ffi::Op::Push(*i),
        OpIn::Sing { i, prev } => ffi::Op::Sing { i: *i, prev: *prev },
        OpIn::Dupin { eval, normal } => ffi::Op::Dupin {
            eval: eval.clone(),
            normal: *normal,
        },
        OpIn::Solve { sop, req, tol, best } => ffi::Op::Solve {
            op: *sop,
            req: *req,
            tol: *tol,
            best: *best,
        },
        OpIn::BestSub { a, b } => ffi::Op::BestSub(*a, *b),
        OpIn::Gauss { a, b } => ffi::Op::Gauss(*a, *b),
        OpIn::Local { a, b } => ffi::Op::Local(*a, *b),
        OpIn::Interior => ffi::Op::Interior,
        OpIn::SolveBranch { a, b } => ffi::Op::SolveBranch(*a, *b),
    }
}

fn ffi_program(i: &Inputs) -> ffi::Program {
    ffi::Program {
        a: ffi_surface(&i.a),
        b: i.b.as_ref().map(ffi_surface),
        dom1: i.dom1,
        dom2: i.dom2,
        fillet: i.fillet,
        trees: i.trees.as_ref().map(|(t0, t1)| {
            (
                t0.iter().map(ffi_node).collect(),
                t1.as_ref().map(|t| t.iter().map(ffi_node).collect()),
            )
        }),
        points: i.points.clone(),
        ops: i.ops.iter().map(ffi_op).collect(),
    }
}

// ---- C++ side -----------------------------------------------------------------------------

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    match ffi::run(&ffi_program(i)) {
        Ok(rec) => Ok(Output::new().values("rec", rec)),
        Err(e) => Err(harness(format!("C++: {e}"))),
    }
}

// ---- Rust side ----------------------------------------------------------------------------

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

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// `Ok(v)` or an op status (pushed), or a deferral.
fn st<T>(o: &mut Vec<f64>, r: solidcore::Result<T>) -> Result<Option<T>, Fail> {
    match r {
        Ok(v) => {
            o.push(0.0);
            Ok(Some(v))
        }
        Err(e) => {
            if let Some(d) = Output::unported(&e) {
                return Err(Fail::Deferred(Box::new(d)));
            }
            o.push(status(&e));
            Ok(None)
        }
    }
}

fn b(x: bool) -> f64 {
    if x {
        1.0
    } else {
        0.0
    }
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

fn ext(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

fn rust_surface(s: &SurfaceIn) -> Result<Surface, HarnessError> {
    match s {
        SurfaceIn::Plane {
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
        SurfaceIn::BSpline { p, q, net, u, v } => {
            let net = net
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|c| {
                            let [x, y, z, w] = cpt4(c);
                            CPoint::new(x, y, z, w)
                        })
                        .collect()
                })
                .collect();
            let sur = NurbSurface::new(net, *p, *q, u.clone(), v.clone())
                .map_err(|e| harness(format!("surface: {e}")))?;
            Ok(Surface::from(BSplineSurface::from_nurb(sur, false)))
        }
    }
}

fn link(i: i64) -> Option<NodeId> {
    usize::try_from(i).ok().map(NodeId)
}

/// The cache of `surface` over its natural domain with the hand-built tree `t`.
fn rust_cache(
    surface: Surface,
    t: Option<&[NodeIn]>,
    handle: H,
) -> Result<SurfaceCache<H>, HarnessError> {
    // (runner helper without a kernel in scope)
    let d = surface.natural_uv_domain_if_built();
    let mut cache = SurfaceCache::new(surface, d, SurfaceCacheOptions::default());
    cache.set_surface_handle(Some(handle));
    let Some(t) = t else {
        return Ok(cache);
    };
    let mut tree: Tree<H> = Tree::new(Some(TreeOwner::SurfaceCache));
    let mut patches = Vec::new();
    let mut auxs = Vec::new();
    for (k, n) in t.iter().enumerate() {
        let aux_type = AuxDataType::from_code(i32::from(n.aux))
            .ok_or_else(|| harness(format!("bad aux type {}", n.aux)))?;
        let mut polar_box = PolarBox::new();
        for v in &n.polar {
            polar_box.add_vector(v3(*v), None);
        }
        let data = BezierAux2d {
            uv_domain: ext(n.dom),
            polar_box,
            owning_tree_node: Some(NodeId(k)),
            ..BezierAux2d::default()
        };
        let slot = if aux_type == AuxDataType::BezierSurface {
            let (basis, ivls) = n.pseudo.ok_or_else(|| harness("patch without pseudo box"))?;
            let mut pseudo_box = PseudoBox::new();
            pseudo_box
                .set_basis(v3(basis[0]), v3(basis[1]), v3(basis[2]))
                .map_err(|e| harness(format!("pseudo box: {e}")))?;
            let iv = |i: [f64; 2]| Extent1d::from_min_max(i[0], i[1]);
            pseudo_box.set_intervals(iv(ivls[0]), iv(ivls[1]), iv(ivls[2]));
            patches.push(BezierPatch {
                aux: data,
                pseudo_box,
                ..BezierPatch::default()
            });
            patches.len() - 1
        } else {
            auxs.push(data);
            auxs.len() - 1
        };
        let [x0, y0, z0, x1, y1, z1] = n.bbox;
        tree.add_node(TreeNode {
            parent: link(n.parent),
            child1: link(n.c1),
            child2: link(n.c2),
            bbox: Extent3d::from_points(Vec3::new(x0, y0, z0), Vec3::new(x1, y1, z1)),
            aux_type,
            data: NodeData::Surface(SurfaceAux {
                surface: Some(handle),
                slot: Some(slot),
                ..SurfaceAux::default()
            }),
            geom_type: NodeGeomType::Default,
        });
    }
    tree.set_top_node(Some(NodeId(0)));
    *cache.patches_mut() = patches;
    *cache.auxs_mut() = auxs;
    cache.set_tree(Some(tree));
    Ok(cache)
}

fn ept(code: f64) -> Result<ExtentPointType, HarnessError> {
    Ok(match code as i64 {
        0 => ExtentPointType::Start,
        1 => ExtentPointType::Inside,
        2 => ExtentPointType::End,
        3 => ExtentPointType::Both,
        4 => ExtentPointType::Outside,
        5 => ExtentPointType::Unknown,
        _ => return Err(harness("extent point type")),
    })
}

fn rust_point(v: &[f64]) -> Result<TsectPnt, HarnessError> {
    if v.len() != p1ffi::POINT_LEN {
        return Err(harness("point length"));
    }
    let mut it = v.iter().copied();
    let mut g = || it.next().unwrap_or(f64::NAN);
    let mut t = TsectPnt::new();
    t.point_type =
        IntersectionPointType::from_code(g() as i32).ok_or_else(|| harness("point type"))?;
    for pt in t.uv_param_type.iter_mut().flatten() {
        *pt = ept(g())?;
    }
    for uv in t.uv_curve_pv.iter_mut().flatten() {
        *uv = Vec2::new(g(), g());
    }
    for vec in t.surface_pv.iter_mut().flatten().flatten() {
        *vec = Vec3::new(g(), g(), g());
    }
    for vec in &mut t.curve_pv {
        *vec = Vec3::new(g(), g(), g());
    }
    t.curve_parameter = g();
    t.tangent_plane_angle_rad = g();
    t.deviation = g();
    for u in &mut t.user_doubles {
        *u = g();
    }
    Ok(t)
}

fn put_vec(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_uv(o: &mut Vec<f64>, v: Vec2) {
    o.extend([v.x, v.y]);
}

fn put_point(o: &mut Vec<f64>, t: &TsectPnt) {
    o.push(f64::from(t.point_type.code()));
    o.extend(t.uv_param_type.iter().flatten().map(|pt| f64::from(pt.code())));
    for uv in t.uv_curve_pv.iter().flatten() {
        put_uv(o, *uv);
    }
    for vec in t.surface_pv.iter().flatten().flatten() {
        put_vec(o, *vec);
    }
    put_vec(o, t.curve_pv[0]);
    put_vec(o, t.curve_pv[1]);
    o.extend([t.curve_parameter, t.tangent_plane_angle_rad, t.deviation]);
    o.extend(t.user_doubles);
}

fn node_index(n: Option<NodeId>) -> f64 {
    n.map_or(-1.0, |id| id.0 as f64)
}

fn put_solutions(o: &mut Vec<f64>, a: &SolutionArray<H>) {
    o.push(a.len() as f64);
    for s in a.as_slice() {
        o.push(f64::from(s.solution_type as i32));
        o.push(s.num_variables as f64);
        o.push(s.start.solution_value);
        o.extend(s.start.parameters);
        o.push(s.end.solution_value);
        o.extend(s.end.parameters);
        o.push(node_index(s.nodes[0]));
        o.push(node_index(s.nodes[1]));
    }
}

type Si<'a> = AdvSurfaceIntersector<'a, 'a, H>;

fn put_state(o: &mut Vec<f64>, si: &Si<'_>) {
    o.push(si.closest_value());
    let uvs = si.closest_uvs();
    put_uv(o, uvs[0]);
    put_uv(o, uvs[1]);
    let nodes = si.closest_nodes();
    o.push(node_index(nodes[0]));
    o.push(node_index(nodes[1]));
    o.extend([
        b(si.separate_gauss_maps()),
        si.base.curve_trace_direction,
        si.base.this_ang_tol_rad,
        si.base.this_approx_tol_3d,
        si.solver.tolerance_3d,
        f64::from(si.solver.operation as i32),
        si.base.curve_points().len() as f64,
    ]);
}

fn sop_of(code: u8) -> Result<SolverOperation, HarnessError> {
    SolverOperation::from_code(i32::from(code)).ok_or_else(|| harness("solver operation"))
}

fn branch(a: usize, b: usize) -> [Option<NodeId>; MAX_SOLVER_TREES] {
    [Some(NodeId(a)), Some(NodeId(b)), None]
}

/// True for the error `ComputeSingularityPoint` returns on a ninth singular direction.
fn is_sing_overflow(e: &SolidError) -> bool {
    matches!(e, SolidError::InvalidInput { reason, .. } | SolidError::PortInvariant { reason, .. } if reason.starts_with("more than 8 singular"))
}

struct Ctx<'a> {
    k: &'a Kernel,
    aux: [Vec<AuxDataType>; 2],
    sols: SolutionArray<H>,
}

#[allow(clippy::too_many_lines)] // one match over the ops
fn run_op(k: &Kernel, cx: &mut Ctx<'_>, si: &mut Si<'_>, op: &OpIn, o: &mut Vec<f64>) -> Result<(), Fail> {
    match op {
        OpIn::State => {
            o.push(0.0);
            put_state(o, si);
        }
        OpIn::Set {
            dir,
            ang,
            approx,
            tol,
            sop,
        } => {
            si.base.curve_trace_direction = *dir;
            si.base.this_ang_tol_rad = *ang;
            si.base.this_approx_tol_3d = *approx;
            si.solver.tolerance_3d = *tol;
            si.solver.operation = sop_of(*sop)?;
            o.push(0.0);
        }
        OpIn::Push { i } => {
            if si.base.is_linked(TsectId(*i)) {
                o.push(ffi::REJECTED);
            } else {
                st(o, si.base.add_point_to_curve(TsectId(*i)))?;
            }
        }
        OpIn::Sing { i, prev } => {
            let id = TsectId(*i);
            let uvs = {
                let p = si
                    .base
                    .point(id)
                    .map_err(|e| Fail::Harness(harness(format!("point: {e}"))))?;
                [
                    p.uv_pos(SrfIndex::First),
                    p.uv_pos(SrfIndex::Second),
                ]
            };
            let r = si.compute_singularity_point(uvs, id, prev.map(TsectId));
            if let Err(e) = &r {
                if is_sing_overflow(e) {
                    o.push(ffi::REJECTED);
                    return Ok(());
                }
            }
            st(o, r)?;
            let p = si
                .base
                .point(id)
                .map_err(|e| Fail::Harness(harness(format!("point: {e}"))))?;
            put_point(o, p);
        }
        OpIn::Dupin { eval, normal } => {
            let mut m = [[Vec3::UNDEF; 3]; 3];
            for (row, src) in m.iter_mut().zip(eval.iter()) {
                for (v, s) in row.iter_mut().zip(src) {
                    *v = v3(*s);
                }
            }
            let r = si.make_dupin(&mut m, v3(*normal));
            let d = st(o, r)?;
            for v in m.iter().flatten() {
                put_vec(o, *v);
            }
            if let Some(d) = d {
                put_vec(o, d.prin_k1_vec);
                put_vec(o, d.prin_k2_vec);
                o.extend([d.prin_k1, d.prin_k2]);
                put_vec(o, d.efg);
                put_vec(o, d.lmn);
                for list in [&d.above, &d.below] {
                    o.push(list.len() as f64);
                    for c in list {
                        o.extend([c.conic_type().code() as f64, c.a(), c.b()]);
                        put_uv(o, c.origin());
                        o.push(c.theta_rad());
                    }
                }
            }
        }
        OpIn::Solve { sop, req, tol, best } => {
            let mut a = SolutionArray::new();
            let request = SolveRequest {
                operation: sop_of(*sop)?,
                solution_requested: SolutionRequested::from_code(i32::from(*req))
                    .ok_or_else(|| harness("solution requested"))?,
                tolerance_3d: *tol,
                best_answer_so_far_sq: *best,
                opt_vectors: &[],
            };
            let r = si.solve_it(&request, &mut a);
            st(o, r)?;
            put_solutions(o, &a);
        }
        OpIn::BestSub { a, b: bb } => {
            let r = si.find_best_subdivision_node(&branch(*a, *bb));
            let v = st(o, r)?;
            let (idx, swap) = v.unwrap_or((99, false));
            o.extend([idx as f64, b(swap)]);
        }
        OpIn::Gauss { a, b: bb } => {
            let r = si.branch_is_gauss_separate(k, &branch(*a, *bb));
            let v = st(o, r)?;
            o.push(b(v.unwrap_or(false)));
        }
        OpIn::Local { a, b: bb } => {
            let is_patch = |t: usize, i: usize| {
                cx.aux.get(t).and_then(|a| a.get(i)).copied() == Some(AuxDataType::BezierSurface)
            };
            if !is_patch(0, *a) || !is_patch(1, *bb) {
                o.push(ffi::REJECTED);
                return Ok(());
            }
            std::mem::swap(&mut si.solver.solutions, &mut cx.sols);
            let r = si.local_solve(&branch(*a, *bb));
            std::mem::swap(&mut si.solver.solutions, &mut cx.sols);
            let more = st(o, r)?;
            o.push(b(more.unwrap_or(false)));
            put_solutions(o, &cx.sols);
            put_state(o, si);
        }
        OpIn::Interior => {
            let mut out = InteriorCurves::default();
            let r = si.find_interior_curves(cx.k, &mut out);
            st(o, r)?;
            o.push(out.curves_3d.len() as f64);
            put_state(o, si);
        }
        OpIn::SolveBranch { a, b: bb } => {
            std::mem::swap(&mut si.solver.solutions, &mut cx.sols);
            let r = si.solve_branch(&branch(*a, *bb));
            std::mem::swap(&mut si.solver.solutions, &mut cx.sols);
            st(o, r)?;
            put_solutions(o, &cx.sols);
            put_state(o, si);
        }
    }
    Ok(())
}

fn rust_inner(i: &Inputs) -> Result<Vec<f64>, Fail> {
    let k = Kernel::default();
    let sa = rust_surface(&i.a)?;
    let (t0, t1) = match &i.trees {
        Some((t0, t1)) => (Some(t0.as_slice()), t1.as_deref()),
        None => (None, None),
    };
    let cache_a = rust_cache(sa, t0, H(0))?;
    let cache_b = match &i.b {
        Some(sb) => Some(rust_cache(rust_surface(sb)?, t1, H(1))?),
        None => None,
    };
    let cb = cache_b.as_ref().unwrap_or(&cache_a);
    let aux_of = |t: Option<&[NodeIn]>| -> Vec<AuxDataType> {
        t.unwrap_or(&[])
            .iter()
            .filter_map(|n| AuxDataType::from_code(i32::from(n.aux)))
            .collect()
    };
    let mut cx = Ctx {
        k: &k,
        aux: [aux_of(t0), aux_of(if i.b.is_some() { t1 } else { t0 })],
        sols: SolutionArray::new(),
    };

    let mut si: Si<'_> = match AdvSurfaceIntersector::new(
        &k, cache_a.surface(),
        &ext(i.dom1),
        cb.surface(),
        &ext(i.dom2),
        i.fillet,
    ) {
        Ok(si) => si,
        Err(e) => {
            return Err(match Output::unported(&e) {
                Some(d) => Fail::Deferred(Box::new(d)),
                None => Fail::Harness(harness(format!("constructor: {e}"))),
            })
        }
    };
    if i.trees.is_some() {
        si.set_surface_caches(&cache_a, cb)
            .map_err(|e| Fail::Harness(harness(format!("caches: {e}"))))?;
    }
    for p in &i.points {
        si.base.alloc_point(rust_point(p)?);
    }
    let mut o = Vec::new();
    for op in &i.ops {
        o.push(op_code(op));
        run_op(&k, &mut cx, &mut si, op, &mut o)?;
    }
    put_state(&mut o, &si);
    Ok(o)
}

fn op_code(op: &OpIn) -> f64 {
    match op {
        OpIn::State => 1.0,
        OpIn::Set { .. } => 2.0,
        OpIn::Push { .. } => 3.0,
        OpIn::Sing { .. } => 4.0,
        OpIn::Dupin { .. } => 5.0,
        OpIn::Solve { .. } => 6.0,
        OpIn::BestSub { .. } => 7.0,
        OpIn::Gauss { .. } => 8.0,
        OpIn::Local { .. } => 9.0,
        OpIn::Interior => 10.0,
        OpIn::SolveBranch { .. } => 11.0,
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    match rust_inner(i) {
        Ok(rec) => Ok(Output::new().values("rec", rec)),
        Err(Fail::Deferred(d)) => Ok(*d),
        Err(Fail::Harness(e)) => Err(e),
    }
}

// ---- generation -----------------------------------------------------------------------------

type V = [f64; 3];

fn add(a: V, c: V) -> V {
    [a[0] + c[0], a[1] + c[1], a[2] + c[2]]
}

fn mul(s: f64, a: V) -> V {
    [s * a[0], s * a[1], s * a[2]]
}

fn dot(a: V, c: V) -> f64 {
    a[0] * c[0] + a[1] * c[1] + a[2] * c[2]
}

fn cross(a: V, c: V) -> V {
    [
        a[1] * c[2] - a[2] * c[1],
        a[2] * c[0] - a[0] * c[2],
        a[0] * c[1] - a[1] * c[0],
    ]
}

fn norm(a: V) -> V {
    let l = dot(a, a).sqrt();
    if l > 0.0 {
        mul(1.0 / l, a)
    } else {
        [0.0, 0.0, 1.0]
    }
}

fn unit_vec(rng: &mut Rng) -> V {
    loop {
        let v = rng.point3(-1.0, 1.0);
        let l = dot(v, v).sqrt();
        if l > 0.2 {
            return mul(1.0 / l, v);
        }
    }
}

/// A unit vector, sometimes axis aligned.
fn direction(rng: &mut Rng) -> V {
    if rng.chance(0.3) {
        let sign = if rng.chance(0.5) { 1.0 } else { -1.0 };
        match rng.below(3) {
            0 => [sign, 0.0, 0.0],
            1 => [0.0, sign, 0.0],
            _ => [0.0, 0.0, sign],
        }
    } else {
        unit_vec(rng)
    }
}

/// A curvature magnitude: mostly moderate, sometimes tiny, huge or at a threshold.
fn curvature(rng: &mut Rng) -> f64 {
    let m = match rng.below(12) {
        0 => 1e-7 * rng.uniform(0.5, 2.0),
        1 => 1e-6 * rng.uniform(0.9, 1.1),
        2 => 1e-12 * rng.uniform(0.5, 2.0),
        3 => 1e4 * rng.uniform(0.5, 2.0),
        _ => 10f64.powf(rng.uniform(-2.0, 2.0)),
    };
    if rng.chance(0.5) {
        m
    } else {
        -m
    }
}

/// A symmetric shape tensor `[s11, s12, s22]` in the tangent frame.
fn shape(rng: &mut Rng) -> [f64; 3] {
    let t = rng.uniform(0.0, core::f64::consts::PI);
    let (c, s) = (t.cos(), t.sin());
    let (k1, k2) = match rng.below(7) {
        0 => (0.0, 0.0),
        1 => (curvature(rng), 0.0),
        2 => {
            let k = curvature(rng);
            (k, k)
        }
        3 => {
            let k = curvature(rng);
            (k, -k * rng.uniform(0.2, 5.0))
        }
        4 => {
            let k = curvature(rng);
            (k, k * rng.uniform(0.2, 5.0))
        }
        5 => {
            let k = curvature(rng);
            (k, k * (1.0 + 1e-13))
        }
        _ => (curvature(rng), curvature(rng)),
    };
    [
        k1 * c * c + k2 * s * s,
        (k1 - k2) * c * s,
        k1 * s * s + k2 * c * c,
    ]
}

/// The evaluation matrix of a surface at `pos` with tangent frame `e1, e2`, normal `n`
/// (stored as `[2][2]`), shape tensor `sh` (relative to `n`) and parametrisation `jac`
/// (`du = j00 e1 + j10 e2`, `dv = j01 e1 + j11 e2`).
fn eval_matrix(
    rng: &mut Rng,
    pos: V,
    e1: V,
    e2: V,
    n: V,
    sh: [f64; 3],
    jac: [[f64; 2]; 2],
) -> [[V; 3]; 3] {
    let du2 = [jac[0][0], jac[1][0]];
    let dv2 = [jac[0][1], jac[1][1]];
    let form = |a: [f64; 2], c: [f64; 2]| {
        a[0] * (sh[0] * c[0] + sh[1] * c[1]) + a[1] * (sh[1] * c[0] + sh[2] * c[1])
    };
    let l = form(du2, du2);
    let m = form(du2, dv2);
    let nn = form(dv2, dv2);
    let du = add(mul(du2[0], e1), mul(du2[1], e2));
    let dv = add(mul(dv2[0], e1), mul(dv2[1], e2));
    let tang = |rng: &mut Rng| {
        if rng.chance(0.4) {
            [0.0; 3]
        } else {
            add(mul(rng.uniform(-1.0, 1.0), e1), mul(rng.uniform(-1.0, 1.0), e2))
        }
    };
    let duu = add(tang(rng), mul(l, n));
    let duv = add(tang(rng), mul(m, n));
    let dvv = add(tang(rng), mul(nn, n));
    let undef = [solidcore::math::UNDEF_DOUBLE; 3];
    [[pos, dv, dvv], [du, duv, undef], [duu, undef, n]]
}

fn jacobian(rng: &mut Rng) -> [[f64; 2]; 2] {
    let s = 10f64.powf(rng.uniform(-2.0, 2.0));
    loop {
        let j = if rng.chance(0.3) {
            [[s, 0.0], [0.0, s * rng.uniform(0.5, 2.0)]]
        } else {
            [
                [s * rng.uniform(-1.0, 1.0), s * rng.uniform(-1.0, 1.0)],
                [s * rng.uniform(-1.0, 1.0), s * rng.uniform(-1.0, 1.0)],
            ]
        };
        let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
        if det.abs() > 0.05 * s * s {
            return j;
        }
    }
}

/// 80 values of a point (layout of the ffi module).
fn point_values(
    ptype: u8,
    uvs: [[f64; 2]; 2],
    srf: &[[[V; 3]; 3]; 2],
    crv_pos: V,
    crv_deriv: V,
) -> Vec<f64> {
    let mut v = vec![f64::from(ptype), 1.0, 1.0, 1.0, 1.0];
    for uv in uvs {
        v.extend(uv);
        v.extend([0.0, 0.0]);
    }
    for s in srf {
        for row in s {
            for x in row {
                v.extend(x);
            }
        }
    }
    v.extend(crv_pos);
    v.extend(crv_deriv);
    v.extend([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    v
}

fn gen_plane(rng: &mut Rng, s: f64) -> SurfaceIn {
    let o = rng.point3(-s, s);
    let x = unit_vec(rng);
    let y = norm(cross(cross(x, unit_vec(rng)), x));
    let y = if dot(y, y) > 0.5 {
        y
    } else {
        norm(cross(x, [0.3, 0.1, 1.0]))
    };
    SurfaceIn::Plane {
        o,
        x,
        y,
        scale: [1.0, 1.0],
        domain: [0.0, 0.0, s, s],
    }
}

/// A plane through `o` with axes `x`, `y`.
fn plane_at(o: V, x: V, y: V, s: f64) -> SurfaceIn {
    SurfaceIn::Plane {
        o,
        x,
        y,
        scale: [1.0, 1.0],
        domain: [0.0, 0.0, s, s],
    }
}

fn gen_bspline(rng: &mut Rng, s: f64, origin: V) -> SurfaceIn {
    let p = 1 + rng.below(2) as usize;
    let q = 1 + rng.below(2) as usize;
    let nu = p + 1 + rng.below(2) as usize;
    let nv = q + 1 + rng.below(2) as usize;
    let rational = rng.chance(0.15);
    let ax = unit_vec(rng);
    let ay = norm(cross(cross(ax, unit_vec(rng)), ax));
    let az = cross(ax, ay);
    let mut net = Vec::new();
    for i in 0..nu {
        let mut row = Vec::new();
        for j in 0..nv {
            let pt = add(
                origin,
                add(
                    add(
                        mul(s * i as f64 / (nu - 1) as f64, ax),
                        mul(s * j as f64 / (nv - 1) as f64, ay),
                    ),
                    mul(s * rng.uniform(-0.3, 0.3), az),
                ),
            );
            let w = if rational {
                Some(rng.uniform(0.5, 2.0))
            } else {
                None
            };
            let ww = w.unwrap_or(1.0);
            row.push([Some(pt[0] * ww), Some(pt[1] * ww), Some(pt[2] * ww), w]);
        }
        net.push(row);
    }
    let knots = |n: usize, p: usize| {
        let mut k = vec![0.0; p + 1];
        for i in 0..(n - p - 1) {
            k.push((i as f64 + 1.0) / (n - p) as f64);
        }
        k.extend(vec![1.0; p + 1]);
        k
    };
    SurfaceIn::BSpline {
        p,
        q,
        net,
        u: knots(nu, p),
        v: knots(nv, q),
    }
}

fn natural(s: &SurfaceIn) -> [f64; 4] {
    match s {
        SurfaceIn::Plane { domain, .. } => *domain,
        SurfaceIn::BSpline { u, v, .. } => [
            u.first().copied().unwrap_or(0.0),
            v.first().copied().unwrap_or(0.0),
            u.last().copied().unwrap_or(1.0),
            v.last().copied().unwrap_or(1.0),
        ],
    }
}

fn gen_domain(rng: &mut Rng, nat: [f64; 4]) -> [f64; 4] {
    let [a, c, e, d] = nat;
    let (w, h) = (e - a, d - c);
    match rng.below(6) {
        0 => [a + 0.1 * w, c + 0.2 * h, e - 0.3 * w, d - 0.1 * h],
        1 => [a + 0.5 * w, c, e, d],
        2 => [a - 0.2 * w, c - 0.1 * h, e + 0.2 * w, d],
        _ => nat,
    }
}

// ---- singular point programs ---------------------------------------------------------------

/// A surface pair at a point with parallel normals and its point values.
fn gen_sing_point(rng: &mut Rng, uvs: [[f64; 2]; 2]) -> Vec<f64> {
    let scale = 10f64.powf(rng.uniform(-3.0, 3.0));
    let pos = mul(scale, rng.point3(-1.0, 1.0));
    let n = direction(rng);
    let e1 = norm(cross(n, unit_vec(rng)));
    let e2 = cross(n, e1);
    let sh1 = shape(rng);
    let flip = rng.chance(0.3);
    let n2 = if flip { mul(-1.0, n) } else { n };
    let rel = |s: [f64; 3]| if flip { [-s[0], -s[1], -s[2]] } else { s };
    let sh2 = match rng.below(6) {
        // geometrically coincident
        0 => rel(sh1),
        // sharing the curvature along one principal direction (shift the other)
        1 => {
            let t = rng.uniform(0.0, core::f64::consts::PI);
            let d = curvature(rng);
            let (c, s) = (t.cos(), t.sin());
            rel([sh1[0] + d * s * s, sh1[1] - d * c * s, sh1[2] + d * c * c])
        }
        // the same tensor, rotated
        2 => {
            let t = rng.uniform(0.0, core::f64::consts::PI);
            let (c, s) = (t.cos(), t.sin());
            let [a, bb, d] = sh1;
            rel([
                c * c * a + 2.0 * c * s * bb + s * s * d,
                (d - a) * c * s + bb * (c * c - s * s),
                s * s * a - 2.0 * c * s * bb + c * c * d,
            ])
        }
        _ => shape(rng),
    };
    let j1 = jacobian(rng);
    let m1 = eval_matrix(rng, pos, e1, e2, n, sh1, j1);
    let j2 = jacobian(rng);
    let m2 = eval_matrix(rng, pos, e1, e2, n2, sh2, j2);
    let deriv = if rng.chance(0.5) {
        unit_vec(rng)
    } else {
        [0.0, 0.0, 0.0]
    };
    point_values(
        if rng.chance(0.5) { 5 } else { 1 },
        uvs,
        &[m1, m2],
        pos,
        deriv,
    )
}

fn gen_carrier_planes(rng: &mut Rng) -> (SurfaceIn, Option<SurfaceIn>, [f64; 4], [f64; 4]) {
    let s = rng.uniform(0.5, 4.0);
    let a = gen_plane(rng, s);
    let bsurf = if rng.chance(0.2) {
        None
    } else {
        Some(gen_plane(rng, s))
    };
    let d1 = gen_domain(rng, natural(&a));
    let d2 = gen_domain(rng, natural(bsurf.as_ref().unwrap_or(&a)));
    (a, bsurf, d1, d2)
}

fn gen_uv(rng: &mut Rng, d: [f64; 4]) -> [f64; 2] {
    let pick = |rng: &mut Rng, lo: f64, hi: f64| match rng.below(5) {
        0 => lo,
        1 => hi,
        _ => rng.uniform(lo, hi),
    };
    [pick(rng, d[0], d[2]), pick(rng, d[1], d[3])]
}

fn gen_set(rng: &mut Rng, sop: u8) -> OpIn {
    OpIn::Set {
        dir: if rng.chance(0.7) { 1.0 } else { -1.0 },
        ang: *rng
            .choose(&[20.0f64.to_radians(), 0.01, 0.3, 1.0])
            .unwrap_or(&0.3),
        approx: 10f64.powf(rng.uniform(-6.0, 0.0)),
        tol: *rng.choose(&[0.0, 1e-6, 1e-3, 0.1]).unwrap_or(&0.0),
        sop,
    }
}

fn gen_sing_case(rng: &mut Rng) -> Inputs {
    let (a, bsurf, dom1, dom2) = gen_carrier_planes(rng);
    let mut points = Vec::new();
    let mut ops = Vec::new();
    if rng.chance(0.6) {
        ops.push(gen_set(rng, 0));
    }
    let nsing = 1 + rng.below(3) as usize;
    for _ in 0..nsing {
        let uvs = [gen_uv(rng, dom1), gen_uv(rng, dom2)];
        let i = points.len();
        points.push(gen_sing_point(rng, uvs));
        let prev = match rng.below(4) {
            0 => {
                // a previous point with a chosen tangent
                let j = points.len();
                let mut pv = points.get(i).cloned().unwrap_or_default();
                let d = if rng.chance(0.15) {
                    [0.0, 0.0, 0.0]
                } else {
                    unit_vec(rng)
                };
                if let Some(slot) = pv.get_mut(70..73) {
                    slot.copy_from_slice(&d);
                }
                points.push(pv);
                Some(j)
            }
            1 => Some(i),
            _ => None,
        };
        if rng.chance(0.35) {
            // a curve point before: GetLastNode() is set
            let j = points.len();
            let mut pv = points.get(i).cloned().unwrap_or_default();
            let d = unit_vec(rng);
            if let Some(slot) = pv.get_mut(70..73) {
                slot.copy_from_slice(&d);
            }
            points.push(pv);
            ops.push(OpIn::Push { i: j });
        }
        ops.push(OpIn::Sing { i, prev });
    }
    ops.push(OpIn::State);
    Inputs {
        a,
        b: bsurf,
        dom1,
        dom2,
        fillet: false,
        trees: None,
        points,
        ops,
    }
}

// ---- Dupin programs ---------------------------------------------------------------------------

fn gen_dupin_case(rng: &mut Rng) -> Inputs {
    let (a, bsurf, dom1, dom2) = gen_carrier_planes(rng);
    let mut ops = Vec::new();
    for _ in 0..=rng.below(4) {
        let n = direction(rng);
        let e1 = norm(cross(n, unit_vec(rng)));
        let e2 = cross(n, e1);
        let pos = rng.point3(-10.0, 10.0);
        let sh = shape(rng);
        let jac = jacobian(rng);
        let mut m = eval_matrix(rng, pos, e1, e2, n, sh, jac);
        match rng.below(10) {
            0 => m[1][0] = [0.0; 3],                 // zero du: rebuilt from dv and n
            1 => m[0][1] = mul(2.0, m[1][0]),        // parallel tangents: n from [2][2]
            2 => {
                m[0][1] = mul(2.0, m[1][0]);
                m[2][2] = [0.0; 3]; // singular point
            }
            3 => m[2][2] = mul(-1.0, m[2][2]),       // stored normal opposite du x dv
            _ => {}
        }
        let normal = match rng.below(4) {
            0 => mul(-1.0, n),
            1 => unit_vec(rng),
            _ => n,
        };
        ops.push(OpIn::Dupin {
            eval: Box::new(m),
            normal,
        });
    }
    Inputs {
        a,
        b: bsurf,
        dom1,
        dom2,
        fillet: false,
        trees: None,
        points: Vec::new(),
        ops,
    }
}

// ---- tree programs ------------------------------------------------------------------------------

/// Samples `s` on a 3 x 3 grid of `d`.
fn samples(k: &Kernel, s: &Surface, d: [f64; 4]) -> Vec<V> {
    let mut v = Vec::new();
    for i in 0..3 {
        for j in 0..3 {
            let uv = Point2::new(
                d[0] + (d[2] - d[0]) * f64::from(i) / 2.0,
                d[1] + (d[3] - d[1]) * f64::from(j) / 2.0,
            );
            if let Ok(p) = s.evaluate_point(k, uv) {
                v.push([p.x, p.y, p.z]);
            }
        }
    }
    v
}

/// Approximate normals of `s` over `d` (finite differences).
fn normals(s: &Surface, d: [f64; 4], count: usize) -> Vec<V> {
    let mut out = Vec::new();
    let h = 1e-4 * ((d[2] - d[0]).abs() + (d[3] - d[1]).abs()).max(1e-9);
    for k in 0..count {
        let t = (k as f64 + 0.5) / count as f64;
        let u = d[0] + (d[2] - d[0]) * t;
        let v = d[1] + (d[3] - d[1]) * (1.0 - t);
        let ev = |u: f64, v: f64| s.evaluate_point(&Kernel::default(), Point2::new(u, v)).ok();
        if let (Some(p), Some(pu), Some(pv)) = (ev(u, v), ev(u + h, v), ev(u, v + h)) {
            let n = cross(
                [pu.x - p.x, pu.y - p.y, pu.z - p.z],
                [pv.x - p.x, pv.y - p.y, pv.z - p.z],
            );
            if dot(n, n) > 1e-30 {
                out.push(norm(n));
            }
        }
    }
    if out.is_empty() {
        out.push([0.0, 0.0, 1.0]);
    }
    out
}

fn bounds(pts: &[V], basis: &[V; 3], grow: f64) -> [[f64; 2]; 3] {
    let mut r = [[f64::MAX, -f64::MAX]; 3];
    for p in pts {
        for (iv, bv) in r.iter_mut().zip(basis) {
            let x = dot(*p, *bv);
            iv[0] = iv[0].min(x);
            iv[1] = iv[1].max(x);
        }
    }
    for iv in &mut r {
        if iv[0] > iv[1] {
            *iv = [0.0, 0.0];
        }
        iv[0] -= grow;
        iv[1] += grow;
    }
    r
}

struct TreeGen<'a> {
    s: &'a Surface,
    nodes: Vec<NodeIn>,
    frame: [V; 3],
}

impl TreeGen<'_> {
    /// Adds the node of domain `d` under `parent` and its subtree; returns its index.
    fn add(&mut self, rng: &mut Rng, parent: i64, d: [f64; 4], depth: u32, aux: u8) -> i64 {
        let k = self.nodes.len();
        let pts = samples(&Kernel::default(), self.s, d);
        let ident = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let grow = 1e-3 * (1.0 + pts.iter().map(|p| dot(*p, *p).sqrt()).fold(0.0, f64::max));
        let bb = bounds(&pts, &ident, grow);
        let basis = if rng.chance(0.5) { ident } else { self.frame };
        let pseudo = (aux == 3).then(|| (basis, bounds(&pts, &basis, grow)));
        let polar = normals(self.s, d, 1 + rng.below(3) as usize);
        self.nodes.push(NodeIn {
            parent,
            c1: -1,
            c2: -1,
            aux,
            bbox: [bb[0][0], bb[1][0], bb[2][0], bb[0][1], bb[1][1], bb[2][1]],
            dom: d,
            polar,
            pseudo,
        });
        let leaf = depth == 0;
        if !leaf {
            let split_u = rng.chance(0.5);
            let t = rng.uniform(0.3, 0.7);
            let (da, db) = if split_u {
                let m = d[0] + (d[2] - d[0]) * t;
                ([d[0], d[1], m, d[3]], [m, d[1], d[2], d[3]])
            } else {
                let m = d[1] + (d[3] - d[1]) * t;
                ([d[0], d[1], d[2], m], [d[0], m, d[2], d[3]])
            };
            // children: patches, or (under a patch, sometimes) tessellation aux nodes
            let child_aux = if aux == 3 && depth >= 2 && rng.chance(0.2) {
                6
            } else {
                3
            };
            let c1 = self.add(rng, k as i64, da, depth - 1, child_aux);
            let c2 = self.add(rng, k as i64, db, depth - 1, child_aux);
            if let Some(n) = self.nodes.get_mut(k) {
                n.c1 = c1;
                n.c2 = c2;
            }
        } else if aux != 3 {
            // a leaf must be a patch
            if let Some(n) = self.nodes.get_mut(k) {
                n.aux = 3;
                n.pseudo = Some((ident, bb));
            }
        }
        k as i64
    }
}

fn gen_tree(rng: &mut Rng, s: &Surface, nat: [f64; 4], depth: u32) -> Vec<NodeIn> {
    let frame = {
        let x = unit_vec(rng);
        let y = norm(cross(cross(x, unit_vec(rng)), x));
        [x, y, cross(x, y)]
    };
    let mut g = TreeGen {
        s,
        nodes: Vec::new(),
        frame,
    };
    let top_aux = if rng.chance(0.6) { 4 } else { 3 };
    g.add(rng, -1, nat, depth, top_aux);
    g.nodes
}

#[allow(clippy::too_many_lines)] // one generator
fn gen_tree_case(rng: &mut Rng) -> Option<Inputs> {
    let s = rng.uniform(0.5, 4.0) * if rng.chance(0.1) { 1e-3 } else { 1.0 };
    let s = if rng.chance(0.05) { s * 1e4 } else { s };
    let kind = rng.below(5);
    let (a, bsurf) = match kind {
        // two planes crossing
        0 => (gen_plane(rng, s), Some(gen_plane(rng, s))),
        // near-parallel overlapping planes (interior curves skip them)
        1 => {
            let o = rng.point3(-s, s);
            let x = unit_vec(rng);
            let y = norm(cross(cross(x, unit_vec(rng)), x));
            let ang = rng.uniform(-4.0, 4.0).to_radians();
            let z = cross(x, y);
            let y2 = add(mul(ang.cos(), y), mul(ang.sin(), z));
            let o2 = add(o, mul(s * rng.uniform(-1e-3, 1e-3), z));
            (plane_at(o, x, y, s), Some(plane_at(o2, x, y2, s)))
        }
        // a B-spline and a plane through it
        2 => {
            let o = rng.point3(-s, s);
            let bs = gen_bspline(rng, s, o);
            (bs, Some(gen_plane(rng, s)))
        }
        // two B-splines
        3 => {
            let o = rng.point3(-s, s);
            let b1 = gen_bspline(rng, s, o);
            let off = unit_vec(rng);
            let b2 = gen_bspline(rng, s, add(o, mul(0.2 * s, off)));
            (b1, Some(b2))
        }
        // self intersection
        _ => {
            let o = rng.point3(-s, s);
            if rng.chance(0.5) {
                (gen_bspline(rng, s, o), None)
            } else {
                (gen_plane(rng, s), None)
            }
        }
    };
    let ra = rust_surface(&a).ok()?;
    let rb = match &bsurf {
        Some(x) => Some(rust_surface(x).ok()?),
        None => None,
    };
    let na = natural(&a);
    let depth_a = rng.below(4) as u32;
    let depth_b = if rng.chance(0.7) {
        depth_a
    } else {
        rng.below(4) as u32
    };
    let ta = gen_tree(rng, &ra, na, depth_a);
    let tb = match (&rb, &bsurf) {
        (Some(r), Some(x)) => Some(gen_tree(rng, r, natural(x), depth_b)),
        _ => None,
    };
    let dom1 = gen_domain(rng, na);
    let dom2 = gen_domain(rng, natural(bsurf.as_ref().unwrap_or(&a)));
    let n0 = ta.len();
    let n1 = tb.as_ref().map_or(n0, Vec::len);
    let pick = |rng: &mut Rng, n: usize| rng.below(n as u64) as usize;

    let mut ops = Vec::new();
    if rng.chance(0.5) {
        let sop = *rng.choose(&[0u8, 2, 3]).unwrap_or(&0);
        ops.push(gen_set(rng, sop));
    }
    let nops = 1 + rng.below(5);
    for _ in 0..nops {
        let op = match rng.below(12) {
            0..=3 => OpIn::Solve {
                sop: *rng.choose(&[0u8, 0, 2, 3]).unwrap_or(&0),
                req: rng.below(4) as u8,
                tol: *rng.choose(&[0.0, 1e-6, 1e-3 * s, 0.1 * s]).unwrap_or(&0.0),
                best: if rng.chance(0.8) {
                    BIG
                } else {
                    rng.uniform(0.0, s)
                },
            },
            4 => OpIn::BestSub {
                a: pick(rng, n0),
                b: pick(rng, n1),
            },
            5 => OpIn::Gauss {
                a: pick(rng, n0),
                b: pick(rng, n1),
            },
            6 | 7 => OpIn::Local {
                a: pick(rng, n0),
                b: pick(rng, n1),
            },
            8 | 9 => OpIn::SolveBranch {
                a: if rng.chance(0.5) { 0 } else { pick(rng, n0) },
                b: if rng.chance(0.5) { 0 } else { pick(rng, n1) },
            },
            10 => {
                if kind == 1 || rng.chance(0.3) {
                    OpIn::Interior
                } else {
                    OpIn::State
                }
            }
            _ => {
                let sop = *rng.choose(&[0u8, 2, 3]).unwrap_or(&0);
                gen_set(rng, sop)
            }
        };
        ops.push(op);
    }
    ops.push(OpIn::State);
    Some(Inputs {
        a,
        b: bsurf,
        dom1,
        dom2,
        fillet: rng.chance(0.1),
        trees: Some((ta, tb)),
        points: Vec::new(),
        ops,
    })
}

fn gen_case(rng: &mut Rng) -> Option<Inputs> {
    match rng.below(20) {
        0..=7 => Some(gen_sing_case(rng)),
        8..=10 => Some(gen_dupin_case(rng)),
        _ => gen_tree_case(rng),
    }
}

// ---- named edge cases -----------------------------------------------------------------------

fn inputs_json(i: &Inputs) -> Value {
    serde_json::to_value(i).unwrap_or(Value::Null)
}

/// The classic configurations of `ComputeSingularityPoint`'s documentation
/// (`source/SMLib/src/SmAdvSurfaceIntersector.cpp:86-111`): a hill touching a plane, a
/// cylinder touching a plane, a saddle on a plane, coincident spheres and two cylinders
/// tangent along a line; at the origin with normal `z`.
fn edge_cases() -> Vec<(String, Value)> {
    let x = [1.0, 0.0, 0.0];
    let y = [0.0, 1.0, 0.0];
    let z = [0.0, 0.0, 1.0];
    let o = [0.0, 0.0, 0.0];
    let undef = [solidcore::math::UNDEF_DOUBLE; 3];
    // surface with du = x, dv = y and second derivatives (L, M, N) along z
    let m = |l: f64, mm: f64, n: f64, nz: f64| -> [[V; 3]; 3] {
        let nv = [0.0, 0.0, nz];
        [
            [o, y, mul(n, nv)],
            [x, mul(mm, nv), undef],
            [mul(l, nv), undef, nv],
        ]
    };
    let plane = plane_at([-1.0, -1.0, 0.0], x, y, 2.0);
    let configs: Vec<Config> = vec![
        ("hill_on_plane", m(0.0, 0.0, 0.0, 1.0), m(1.0, 0.0, 2.0, 1.0), None),
        ("cylinder_on_plane", m(0.0, 0.0, 0.0, 1.0), m(1.0, 0.0, 0.0, 1.0), None),
        (
            "cylinder_on_plane_prev",
            m(0.0, 0.0, 0.0, 1.0),
            m(0.0, 0.0, 1.0, 1.0),
            Some([0.0, -1.0, 0.0]),
        ),
        ("saddle_on_plane", m(0.0, 0.0, 0.0, 1.0), m(1.0, 0.0, -1.0, 1.0), None),
        (
            "saddle_on_plane_opposite_normal",
            m(0.0, 0.0, 0.0, 1.0),
            m(1.0, 0.0, -2.0, -1.0),
            Some([1.0, 1.0, 0.0]),
        ),
        ("plane_on_plane", m(0.0, 0.0, 0.0, 1.0), m(0.0, 0.0, 0.0, -1.0), None),
        ("coincident_spheres", m(1.0, 0.0, 1.0, 1.0), m(1.0, 0.0, 1.0, 1.0), None),
        (
            "coincident_saddles_prev",
            m(1.0, 0.0, -1.0, 1.0),
            m(1.0, 0.0, -1.0, 1.0),
            Some([0.0, 1.0, 0.0]),
        ),
        ("cylinders_tangent_line", m(1.0, 0.0, 0.0, 1.0), m(2.0, 0.0, 0.0, 1.0), None),
        ("nested_bowls", m(1.0, 0.0, 1.0, 1.0), m(2.0, 0.0, 3.0, 1.0), None),
        ("saddle_on_bowl", m(1.0, 0.0, -1.0, 1.0), m(0.5, 0.0, 0.25, 1.0), None),
        ("two_saddles", m(1.0, 0.0, -1.0, 1.0), m(-2.0, 0.5, 1.0, 1.0), None),
        ("cylinder_on_saddle", m(0.0, 1.0, 0.0, 1.0), m(1.0, 0.0, 0.0, 1.0), None),
    ];
    let mut out = Vec::new();
    for (name, m1, m2, prev) in configs {
        for (sfx, last) in [("", false), ("_curve_point", true)] {
            let mut points = vec![point_values(5, [[1.0, 1.0]; 2], &[m1, m2], o, [0.0; 3])];
            let mut ops = vec![];
            let prev_id = prev.map(|d| {
                points.push(point_values(1, [[1.0, 1.0]; 2], &[m1, m2], o, d));
                1
            });
            if last {
                points.push(point_values(1, [[1.0, 1.0]; 2], &[m1, m2], o, [1.0, 0.2, 0.0]));
                ops.push(OpIn::Push {
                    i: points.len() - 1,
                });
            }
            ops.push(OpIn::Sing {
                i: 0,
                prev: prev_id,
            });
            ops.push(OpIn::State);
            let i = Inputs {
                a: plane.clone(),
                b: Some(plane_at([-1.0, -1.0, 0.0], x, y, 2.0)),
                dom1: [-1.0, -1.0, 1.0, 1.0],
                dom2: [-1.0, -1.0, 1.0, 1.0],
                fillet: false,
                trees: None,
                points,
                ops,
            };
            out.push((format!("edge_{name}{sfx}"), inputs_json(&i)));
        }
    }
    // MakeDupin on the same surfaces, against both normals
    let mut ops = Vec::new();
    for (m1, m2) in [
        (m(1.0, 0.0, 2.0, 1.0), m(1.0, 0.0, 0.0, 1.0)),
        (m(1.0, 0.0, -1.0, 1.0), m(0.0, 0.0, 0.0, 1.0)),
        (m(1e-13, 0.0, 1.0, 1.0), m(1.0, 0.0, 1e-4, 1.0)),
    ] {
        for nrm in [z, mul(-1.0, z)] {
            ops.push(OpIn::Dupin {
                eval: Box::new(m1),
                normal: nrm,
            });
            ops.push(OpIn::Dupin {
                eval: Box::new(m2),
                normal: nrm,
            });
        }
    }
    out.extend(size_ratio_cases());
    out.push((
        "edge_dupin_kinds".into(),
        inputs_json(&Inputs {
            a: plane.clone(),
            b: None,
            dom1: [-1.0, -1.0, 1.0, 1.0],
            dom2: [-1.0, -1.0, 1.0, 1.0],
            fillet: false,
            trees: None,
            points: Vec::new(),
            ops,
        }),
    ));
    out
}

/// `LocalSolve` (and a full `SolveIt`) on leaf pairs more than four times apart in
/// size, so the Newton step through `SmSurface::DropPointsToTangentPlane` runs on
/// either surface: one tree is a single patch, the other is subdivided five times.
fn size_ratio_cases() -> Vec<(String, Value)> {
    let mut out = Vec::new();
    let mut rng = Rng::new(0x5121_e7a7);
    let x = [1.0, 0.0, 0.0];
    let y = [0.0, 1.0, 0.0];
    let z = [0.0, 0.0, 1.0];
    let pairs: [(&str, SurfaceIn, SurfaceIn); 3] = [
        (
            "planes",
            plane_at([-1.0, -1.0, 0.0], x, y, 2.0),
            plane_at([-1.0, 0.1, -1.0], x, z, 2.0),
        ),
        (
            "bspline_plane",
            gen_bspline(&mut rng, 2.0, [0.0, 0.0, 0.0]),
            plane_at([-1.0, -1.0, 0.0], x, y, 2.0),
        ),
        (
            "bsplines",
            gen_bspline(&mut rng, 2.0, [0.0, 0.0, 0.0]),
            gen_bspline(&mut rng, 2.0, [0.1, 0.2, 0.0]),
        ),
    ];
    for (name, a, bsurf) in pairs {
        let (Ok(ra), Ok(rb)) = (rust_surface(&a), rust_surface(&bsurf)) else {
            continue;
        };
        for (sfx, deep_first) in [("_small_a", true), ("_small_b", false)] {
            let (da, db) = if deep_first { (5, 0) } else { (0, 5) };
            let ta = gen_tree(&mut rng, &ra, natural(&a), da);
            let tb = gen_tree(&mut rng, &rb, natural(&bsurf), db);
            let leaves = |t: &[NodeIn]| -> Vec<usize> {
                t.iter()
                    .enumerate()
                    .filter(|(_, n)| n.c1 < 0 && n.aux == 3)
                    .map(|(k, _)| k)
                    .collect()
            };
            let (la, lb) = (leaves(&ta), leaves(&tb));
            let mut ops = vec![OpIn::Set {
                dir: 1.0,
                ang: 0.3,
                approx: 1e-3,
                tol: 0.1,
                sop: 0,
            }];
            for &ka in &la {
                for &kb in &lb {
                    ops.push(OpIn::Local { a: ka, b: kb });
                }
            }
            ops.push(OpIn::Set {
                dir: 1.0,
                ang: 0.3,
                approx: 1e-3,
                tol: 1e-3,
                sop: 0,
            });
            ops.push(OpIn::Solve {
                sop: 0,
                req: 1,
                tol: 1e-3,
                best: BIG,
            });
            ops.push(OpIn::State);
            let i = Inputs {
                a: a.clone(),
                b: Some(bsurf.clone()),
                dom1: natural(&a),
                dom2: natural(&bsurf),
                fillet: false,
                trees: Some((ta, Some(tb))),
                points: Vec::new(),
                ops,
            };
            out.push((format!("edge_size_ratio_{name}{sfx}"), inputs_json(&i)));
        }
    }
    out
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.adv_surface_intersector"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let Some(i) = gen_case(rng) else { continue };
            if ffi::encode(&ffi_program(&i)).is_ok() {
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
        let mut v = default_cases(self, &ctx)?;
        for (name, inputs) in edge_cases() {
            v.push(Case {
                unit: self.id().into(),
                name,
                source: CaseSource::Inline,
                inputs,
            });
        }
        Ok(v)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
