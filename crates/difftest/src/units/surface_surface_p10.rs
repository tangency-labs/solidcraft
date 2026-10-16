//! Runner for unit `surface.surface_p10`: `SmLSIGlobalSolver::LocalSolve` and
//! `sm_GlobalCurveIntersectCombineSolutions`.
//!
//! C++: `solidcore_ffi::units::surface_surface_p10` (the library's `SmLSIGlobalSolver` on a
//! hand-built surface cache tree; a verbatim copy of the static combination, see the shim).
//! Rust: `solidcore::surface::surface::p10`.
//!
//! A case is a surface (a real `SmBSplineSurface` or `SmPlane`) and one op:
//!
//! * `lsi`: cache flags, a tree of patch / head nodes (domain, box, continuities, class,
//!   optional polar and pseudo box), the solved node, the solver domain, the line (point,
//!   vector, optional interval, ray firing) and the tolerances. Record: `status [more n
//!   solutions]`.
//! * `combine`: a B-spline curve and interval, the surface domain, the distance tolerance,
//!   curve tree nodes, a surface cache tree and the input solutions. Record: `status [n
//!   solutions]`.
//!
//! Deferred cases: the leaf solve's guesses go through `SmSurface::LocalCurveIntersect`
//! (`surface.surface_p6`), so only the cases that end before it (node outside its face,
//! disjoint or sliver domains, line missing the box or pseudo box, ray behind, interval
//! outside, tangent plane drop failures) compare. The combination defers on
//! `sm_ClassifyCurveSurfaceXSectPoint` (`surface.surface_p4`: a leading point solution, two
//! distant points), `SmSurface::DropPoint` (`surface.surface_p5`: a gap between ranges),
//! `SmSurface::ClassifyIntersectingRangeSolution` (`surface.surface_p6`: a long range with
//! inexact ends) and `SmSurface::DropAndTrimCurve` (`intersect.surface_drop_lib_p1`: a long
//! range). Generation favours the cases that compare.
//!
//! At least [`MIN_GENERATED`] generated cases run.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve;
use solidcore::curve::curve_cache::BezierAux1d;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::geom::global_solver::{Branch, SolutionRequested, SolverOperation};
use solidcore::geom::polar_box::PolarBox;
use solidcore::geom::pseudo_box::PseudoBox;
use solidcore::geom::solution_array::{
    ObjectKind, Solution, SolutionArray, SolutionObject, SolutionType,
};
use solidcore::geom::tree::{
    AuxDataType, CurveSpanRef, NodeClass, NodeData, NodeGeomType, NodeId, SplitDir, SurfaceAux,
    Tree, TreeNode, TreeOwner,
};
use solidcore::geom::vector2d::Vec2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::p10::{global_curve_intersect_combine_solutions, CombineContext};
use solidcore::surface::surface::p2::LsiGlobalSolver;
use solidcore::surface::surface::Surface;
use solidcore::surface::surface_cache::p1::{
    BezierAux2d, BezierPatch, SurfaceCache, SurfaceCacheOptions,
};
use solidcore::SolidError;
use solidcore_ffi::units::surface_surface_p10 as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 800;

/// `surface.surface_p10` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// The objects of the solutions: `0` the curve, `1` the surface.
#[derive(Debug, Clone, Copy, PartialEq)]
struct H(u8);

impl SolutionObject for H {
    fn kind(&self) -> ObjectKind {
        if self.0 == 0 {
            ObjectKind::Curve
        } else {
            ObjectKind::Surface
        }
    }
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
struct PseudoIn {
    basis: [[f64; 3]; 3],
    pts: Vec<[f64; 3]>,
}

fn c0() -> [u8; 2] {
    [2, 2]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NodeIn {
    #[serde(default)]
    parent: Option<usize>,
    #[serde(default)]
    c1: Option<usize>,
    #[serde(default)]
    c2: Option<usize>,
    aux: u8,
    dom: [f64; 4],
    #[serde(default)]
    class: u8,
    bbox: [f64; 6],
    #[serde(default = "c0")]
    u_conts: [u8; 2],
    #[serde(default = "c0")]
    v_conts: [u8; 2],
    #[serde(default)]
    polar: Option<Vec<[f64; 3]>>,
    #[serde(default)]
    pseudo: Option<PseudoIn>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)] // the surface cache's four independent switches
struct Flags {
    #[serde(default)]
    point_test: bool,
    #[serde(default)]
    have_t: bool,
    #[serde(default)]
    modified: bool,
    #[serde(default)]
    boundary: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct CurveNodeIn {
    aux: u8,
    ivl: [f64; 2],
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct SolIn {
    /// `SmSolutionType` code.
    kind: u8,
    #[serde(default = "three")]
    nvars: u8,
    #[serde(default)]
    sv: f64,
    s: [f64; 3],
    #[serde(default)]
    ev: f64,
    #[serde(default)]
    e: [f64; 3],
    #[serde(default)]
    n0: Option<usize>,
    #[serde(default)]
    n1: Option<usize>,
}

fn three() -> u8 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Lsi {
        #[serde(default)]
        flags: Flags,
        nodes: Vec<NodeIn>,
        #[serde(default)]
        node: usize,
        dom: [f64; 4],
        point: [f64; 3],
        vector: [f64; 3],
        #[serde(default)]
        ivl: Option<[f64; 2]>,
        #[serde(default)]
        ray: bool,
        dist_tol: f64,
        tol3d: f64,
    },
    Combine {
        curve: CurveData,
        dom: [f64; 4],
        ivl: [f64; 2],
        tol: f64,
        #[serde(default)]
        curve_nodes: Vec<CurveNodeIn>,
        nodes: Vec<NodeIn>,
        sols: Vec<SolIn>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    surface: SurfIn,
    #[serde(flatten)]
    op: OpIn,
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

fn ffi_node(n: &NodeIn) -> ffi::NodeIn {
    ffi::NodeIn {
        parent: n.parent,
        child1: n.c1,
        child2: n.c2,
        aux: n.aux,
        domain: n.dom,
        class: n.class,
        bbox: n.bbox,
        u_conts: n.u_conts,
        v_conts: n.v_conts,
        polar: n.polar.clone(),
        pseudo: n.pseudo.as_ref().map(|p| ffi::PseudoIn {
            basis: p.basis,
            points: p.pts.clone(),
        }),
    }
}

fn ffi_program(i: &Inputs) -> ffi::Program {
    let op = match &i.op {
        OpIn::Lsi {
            flags,
            nodes,
            node,
            dom,
            point,
            vector,
            ivl,
            ray,
            dist_tol,
            tol3d,
        } => ffi::Op::Lsi(ffi::LsiIn {
            flags: ffi::CacheFlags {
                point_test: flags.point_test,
                have_t_surface_cache: flags.have_t,
                face_was_modified: flags.modified,
                process_boundary_curves: flags.boundary,
            },
            nodes: nodes.iter().map(ffi_node).collect(),
            node: *node,
            uv_domain: *dom,
            point: *point,
            vector: *vector,
            interval: *ivl,
            ray_fire: *ray,
            distance_tolerance: *dist_tol,
            tolerance_3d: *tol3d,
        }),
        OpIn::Combine {
            curve,
            dom,
            ivl,
            tol,
            curve_nodes,
            nodes,
            sols,
        } => ffi::Op::Combine(ffi::CombineIn {
            curve: ffi::CurveData {
                p: curve.p,
                cpts: curve.cpts.clone(),
                knots: curve.knots.clone(),
            },
            uv_domain: *dom,
            interval: *ivl,
            tolerance: *tol,
            curve_nodes: curve_nodes
                .iter()
                .map(|n| ffi::CurveNodeIn {
                    aux: n.aux,
                    interval: n.ivl,
                })
                .collect(),
            surface_nodes: nodes.iter().map(ffi_node).collect(),
            solutions: sols
                .iter()
                .map(|s| ffi::SolutionIn {
                    kind: s.kind,
                    num_variables: s.nvars,
                    start_value: s.sv,
                    start: s.s,
                    end_value: s.ev,
                    end: s.e,
                    node0: s.n0,
                    node1: s.n1,
                })
                .collect(),
        }),
    };
    ffi::Program {
        surface: ffi_surface(&i.surface),
        op,
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

fn ext(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
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

fn rust_curve(d: &CurveData) -> Result<Curve, HarnessError> {
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
    Ok(Curve::from(
        BSplineCurve::from_data(3, n, m, d.p, &pw, &d.knots)
            .map_err(|e| harness(format!("B-spline: {e}")))?,
    ))
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

fn continuity(code: u8) -> Result<ContinuityType, HarnessError> {
    CONTINUITIES
        .get(usize::from(code))
        .copied()
        .ok_or_else(|| harness("continuity code"))
}

fn node_class(code: u8) -> Result<NodeClass, HarnessError> {
    Ok(match code {
        0 => NodeClass::Unknown,
        1 => NodeClass::Inside,
        2 => NodeClass::Outside,
        3 => NodeClass::OnBoundary,
        _ => return Err(harness("node class")),
    })
}

/// The shim's `read_tree` in a cache of `surface` over its natural domain.
fn build_cache(surface: &Surface, nodes: &[NodeIn]) -> Result<SurfaceCache<H>, HarnessError> {
    let handle = H(1);
    let mut cache = SurfaceCache::new(
        surface.clone(),
        surface.natural_uv_domain(),
        SurfaceCacheOptions::default(),
    );
    cache.set_surface_handle(Some(handle));
    let mut tree: Tree<H> = Tree::new(Some(TreeOwner::SurfaceCache));
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
                // the shim ignores the status (the basis stays as it was)
                let _ = pseudo.set_basis(v3(b1), v3(b2), v3(b3));
                for &p in &ps.pts {
                    pseudo.add_point(v3(p));
                }
            }
            patches.push(BezierPatch {
                aux,
                u_curve_conts: [continuity(n.u_conts[0])?, continuity(n.u_conts[1])?],
                v_curve_conts: [continuity(n.v_conts[0])?, continuity(n.v_conts[1])?],
                pseudo_box: pseudo,
                ..BezierPatch::default()
            });
            patches.len() - 1
        } else {
            auxs.push(aux);
            auxs.len() - 1
        };
        let b = n.bbox;
        tree.add_node(TreeNode {
            parent: n.parent.map(NodeId),
            child1: n.c1.map(NodeId),
            child2: n.c2.map(NodeId),
            bbox: Extent3d::from_bounds(b[0], b[1], b[2], b[3], b[4], b[5]),
            aux_type,
            data: NodeData::Surface(SurfaceAux {
                split_dir: SplitDir::Unknown,
                is_marked: false,
                node_class: node_class(n.class)?,
                face: None,
                surface: Some(handle),
                slot: Some(slot),
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

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// The shim's `put_sol`.
fn put_sol(o: &mut Vec<f64>, s: &Solution<H>) {
    o.extend([
        f64::from(s.solution_type.code()),
        s.num_variables as f64,
        s.num_objects as f64,
        s.start.solution_value,
    ]);
    o.extend(s.start.parameters.iter().take(3));
    o.push(s.end.solution_value);
    o.extend(s.end.parameters.iter().take(3));
    for n in s.nodes.iter().take(2) {
        o.push(n.map_or(-1.0, |id| id.0 as f64));
    }
    for obj in s.objects.iter().take(2) {
        o.push(obj.map_or(-1.0, |h| f64::from(h.0)));
    }
}

fn finish(r: solidcore::Result<Vec<f64>>) -> Output {
    match r {
        Ok(rec) => Output::new().values("rec", rec),
        Err(e) => Output::unported(&e)
            .unwrap_or_else(|| Output::new().values("rec", vec![status(&e)])),
    }
}

#[allow(clippy::too_many_arguments)]
fn rust_lsi(
    k: &Kernel,
    surface: &Surface,
    flags: Flags,
    nodes: &[NodeIn],
    node: usize,
    dom: [f64; 4],
    point: [f64; 3],
    vector: [f64; 3],
    ivl: Option<[f64; 2]>,
    ray: bool,
    dist_tol: f64,
    tol3d: f64,
) -> Result<Output, HarnessError> {
    let mut cache = build_cache(surface, nodes)?;
    cache.point_test_enabled = flags.point_test;
    cache.have_t_surface_cache = flags.have_t;
    cache.face_was_modified = flags.modified;
    cache.process_boundary_curves = flags.boundary;
    let tree = cache.tree().ok_or_else(|| harness("surface tree"))?;
    let mut solver = LsiGlobalSolver::new(
        k, tree,
        Some(cache.surface()),
        ext(dom),
        v3(point),
        v3(vector),
        ivl.map(|i| Extent1d::from_min_max(i[0], i[1])),
        ray,
        dist_tol,
    );
    solver.base.set_solver_operation(SolverOperation::Intersect);
    solver.base.set_3d_tolerance(tol3d);
    solver
        .base
        .set_solution_requested(SolutionRequested::All);
    solver.surface_cache = Some(&cache);
    solver.kernel = k;
    let mut branch: Branch = Default::default();
    if let Some(slot) = branch.first_mut() {
        *slot = Some(NodeId(node));
    }
    let r = solver.local_solve_branch(k, &branch).map(|more| {
        let mut o = vec![
            0.0,
            if more { 1.0 } else { 0.0 },
            solver.base.solutions.len() as f64,
        ];
        for s in solver.base.solutions.as_slice() {
            put_sol(&mut o, s);
        }
        o
    });
    Ok(finish(r))
}

#[allow(clippy::too_many_arguments)]
fn rust_combine(
    k: &Kernel,
    surface: &Surface,
    curve: &CurveData,
    dom: [f64; 4],
    ivl: [f64; 2],
    tol: f64,
    curve_nodes: &[CurveNodeIn],
    nodes: &[NodeIn],
    sols: &[SolIn],
) -> Result<Output, HarnessError> {
    let curve = rust_curve(curve)?;
    let mut curve_tree: Tree<H> = Tree::new(Some(TreeOwner::CurveCache));
    let mut parents = Vec::new();
    for (i, n) in curve_nodes.iter().enumerate() {
        let aux_type = AuxDataType::from_code(i32::from(n.aux))
            .ok_or_else(|| harness(format!("bad aux type {}", n.aux)))?;
        parents.push(BezierAux1d {
            interval: Extent1d::from_min_max(n.ivl[0], n.ivl[1]),
            ..BezierAux1d::default()
        });
        curve_tree.add_node(TreeNode {
            aux_type,
            data: NodeData::Curve(CurveSpanRef::Parent(i)),
            ..TreeNode::new()
        });
    }
    curve_tree.set_curve_data(Vec::new(), parents, Some(H(0)));
    let cache = build_cache(surface, nodes)?;
    let mut solutions = SolutionArray::new();
    for s in sols {
        let mut sol = Solution::new();
        sol.solution_type = SolutionType::from_code(i32::from(s.kind))
            .ok_or_else(|| harness("solution type"))?;
        sol.num_variables = usize::from(s.nvars);
        sol.num_objects = 2;
        sol.objects[0] = Some(H(0));
        sol.objects[1] = Some(H(1));
        sol.start.solution_value = s.sv;
        sol.end.solution_value = s.ev;
        for (d, v) in sol.start.parameters.iter_mut().zip(s.s) {
            *d = v;
        }
        for (d, v) in sol.end.parameters.iter_mut().zip(s.e) {
            *d = v;
        }
        sol.nodes[0] = s.n0.map(NodeId);
        sol.nodes[1] = s.n1.map(NodeId);
        solutions.add(&sol);
    }
    let ctx = CombineContext {
        curve_tree: Some(&curve_tree),
        surface_cache: Some(&cache),
        curve_handle: Some(H(0)),
        surface_handle: Some(H(1)),
    };
    let r = global_curve_intersect_combine_solutions(
        k,
        surface,
        &ext(dom),
        &curve,
        &Extent1d::from_min_max(ivl[0], ivl[1]),
        tol,
        &ctx,
        &mut solutions,
    )
    .map(|()| {
        let mut o = vec![0.0, solutions.len() as f64];
        for s in solutions.as_slice() {
            put_sol(&mut o, s);
        }
        o
    });
    Ok(finish(r))
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let surface = rust_surface(&i.surface)?;
    match &i.op {
        OpIn::Lsi {
            flags,
            nodes,
            node,
            dom,
            point,
            vector,
            ivl,
            ray,
            dist_tol,
            tol3d,
        } => rust_lsi(
            &k, &surface, *flags, nodes, *node, *dom, *point, *vector, *ivl, *ray, *dist_tol,
            *tol3d,
        ),
        OpIn::Combine {
            curve,
            dom,
            ivl,
            tol,
            curve_nodes,
            nodes,
            sols,
        } => rust_combine(
            &k,
            &surface,
            curve,
            *dom,
            *ivl,
            *tol,
            curve_nodes,
            nodes,
            sols,
        ),
    }
}

// ---- generation ---------------------------------------------------------------------

fn pick<T: Copy>(rng: &mut Rng, xs: &[T], default: T) -> T {
    rng.choose(xs).copied().unwrap_or(default)
}

fn unit_dir(rng: &mut Rng) -> [f64; 3] {
    loop {
        let v = rng.point3(-1.0, 1.0);
        let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if n > 0.1 && n <= 1.0 {
            return [v[0] / n, v[1] / n, v[2] / n];
        }
    }
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unitize(a: [f64; 3]) -> [f64; 3] {
    let n = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    [a[0] / n, a[1] / n, a[2] / n]
}

/// A clamped knot vector of `n1` control points of degree `p` over `[0, 1]`, uniform.
fn uniform_knots(n1: usize, p: usize) -> Vec<f64> {
    let interior = n1 - p - 1;
    let mut k = vec![0.0; p + 1];
    k.extend((1..=interior).map(|i| i as f64 / (interior + 1) as f64));
    k.extend(vec![1.0; p + 1]);
    k
}

/// A surface given by a height function `z(x, y)` on a net over `[0, 1]^2` (times `scale`).
fn height_surface(
    n1: usize,
    m1: usize,
    p: usize,
    q: usize,
    scale: f64,
    rational: bool,
    z: impl Fn(f64, f64) -> f64,
) -> SurfData {
    let net = (0..n1)
        .map(|i| {
            (0..m1)
                .map(|j| {
                    let x = i as f64 / (n1 - 1) as f64;
                    let y = j as f64 / (m1 - 1) as f64;
                    let w = if rational {
                        1.0 + 0.25 * ((i + j) % 2) as f64
                    } else {
                        NOW
                    };
                    [x * scale, y * scale, z(x, y) * scale, w]
                })
                .collect()
        })
        .collect();
    SurfData {
        p,
        q,
        net,
        u: uniform_knots(n1, p),
        v: uniform_knots(m1, q),
    }
}

fn gen_bspline(rng: &mut Rng, scale: f64) -> SurfData {
    let rational = rng.chance(0.15);
    match rng.below(5) {
        0 => height_surface(3, 3, 1, 1, scale, rational, |_, _| 0.0),
        1 => height_surface(4, 4, 2, 2, scale, rational, |x, y| {
            0.5 - 0.5 * ((x - 0.5) * (x - 0.5) + (y - 0.5) * (y - 0.5))
        }),
        2 => height_surface(3, 3, 2, 2, scale, rational, |x, y| {
            0.5 * ((x - 0.5) * (x - 0.5) - (y - 0.5) * (y - 0.5))
        }),
        3 => height_surface(5, 4, 3, 2, scale, rational, |x, _| 0.3 * x * x),
        _ => {
            let amp = rng.uniform(0.0, 0.5);
            let zs: Vec<f64> = (0..16).map(|_| rng.uniform(-amp, amp)).collect();
            height_surface(4, 4, 3, 3, scale, rational, |x, y| {
                let i = (x * 3.0).round() as usize;
                let j = (y * 3.0).round() as usize;
                zs.get(i * 4 + j).copied().unwrap_or(0.0)
            })
        }
    }
}

/// `b` (`min xyz, max xyz`) grown to contain `p`.
fn grow_box(b: [f64; 6], p: [f64; 3]) -> [f64; 6] {
    let [x0, y0, z0, x1, y1, z1] = b;
    let [x, y, z] = p;
    [x0.min(x), y0.min(y), z0.min(z), x1.max(x), y1.max(y), z1.max(z)]
}

/// The surface, its natural domain and a box containing it.
fn gen_surface(rng: &mut Rng, scale: f64) -> (SurfIn, [f64; 4], [f64; 6]) {
    if rng.chance(0.15) {
        let x = unit_dir(rng);
        let y = unitize(cross(x, unit_dir(rng)));
        let o = rng.point3(-scale, scale);
        let dom = [0.0, 0.0, scale, scale];
        let mut b = [f64::MAX, f64::MAX, f64::MAX, f64::MIN, f64::MIN, f64::MIN];
        for (u, v) in [(0.0, 0.0), (scale, 0.0), (0.0, scale), (scale, scale)] {
            let p = [
                o[0] + u * x[0] + v * y[0],
                o[1] + u * x[1] + v * y[1],
                o[2] + u * x[2] + v * y[2],
            ];
            b = grow_box(b, p);
        }
        (
            SurfIn::Plane {
                o,
                x,
                y,
                scale: [1.0, 1.0],
                domain: dom,
            },
            dom,
            b,
        )
    } else {
        let d = gen_bspline(rng, scale);
        let mut b = [f64::MAX, f64::MAX, f64::MAX, f64::MIN, f64::MIN, f64::MIN];
        for c in d.net.iter().flatten() {
            b = grow_box(b, [c[0], c[1], c[2]]);
        }
        (SurfIn::Bspline(d), [0.0, 0.0, 1.0, 1.0], b)
    }
}

fn sub_domain(rng: &mut Rng, d: [f64; 4]) -> [f64; 4] {
    let (w, h) = (d[2] - d[0], d[3] - d[1]);
    let u0 = d[0] + w * rng.uniform(0.0, 0.5);
    let v0 = d[1] + h * rng.uniform(0.0, 0.5);
    let u1 = u0 + (d[2] - u0) * rng.uniform(0.2, 1.0);
    let v1 = v0 + (d[3] - v0) * rng.uniform(0.2, 1.0);
    [u0, v0, u1, v1]
}

fn axis_basis(rng: &mut Rng) -> [[f64; 3]; 3] {
    if rng.chance(0.6) {
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
    } else {
        let a = unit_dir(rng);
        let b = unitize(cross(a, unit_dir(rng)));
        let c = cross(a, b);
        [a, b, c]
    }
}

fn box_corners(b: [f64; 6]) -> Vec<[f64; 3]> {
    let mut v = Vec::new();
    for i in 0..8 {
        v.push([
            if i & 1 == 0 { b[0] } else { b[3] },
            if i & 2 == 0 { b[1] } else { b[4] },
            if i & 4 == 0 { b[2] } else { b[5] },
        ]);
    }
    v
}

fn cont(rng: &mut Rng) -> u8 {
    pick(rng, &[0, 1, 2, 2, 3, 7, 7, 13], 2)
}

fn patch_node(rng: &mut Rng, dom: [f64; 4], bbox: [f64; 6], parent: Option<usize>) -> NodeIn {
    let pseudo = if rng.chance(0.9) {
        Some(PseudoIn {
            basis: axis_basis(rng),
            pts: box_corners(bbox),
        })
    } else {
        None
    };
    let polar = if rng.chance(0.3) {
        Some(vec![unit_dir(rng), unit_dir(rng)])
    } else {
        None
    };
    NodeIn {
        parent,
        c1: None,
        c2: None,
        aux: 3,
        dom,
        class: rng.below(4) as u8,
        bbox,
        u_conts: [cont(rng), cont(rng)],
        v_conts: [cont(rng), cont(rng)],
        polar,
        pseudo,
    }
}

fn gen_lsi(rng: &mut Rng) -> Inputs {
    let scale = pick(rng, &[1.0, 1.0, 1.0, 10.0, 1e-3, 1e3], 1.0);
    let (surface, nat, bbox) = gen_surface(rng, scale);
    let flags = Flags {
        point_test: rng.chance(0.4),
        have_t: rng.chance(0.5),
        modified: rng.chance(0.2),
        boundary: rng.chance(0.5),
    };
    let (nodes, node) = if rng.chance(0.25) {
        // a head node over two patches split in u
        let um = nat[0] + (nat[2] - nat[0]) * rng.uniform(0.3, 0.7);
        let left = [nat[0], nat[1], um, nat[3]];
        let right = [um, nat[1], nat[2], nat[3]];
        let mut head = patch_node(rng, nat, bbox, None);
        head.aux = 4;
        head.c1 = Some(1);
        head.c2 = Some(2);
        (
            vec![
                head,
                patch_node(rng, left, bbox, Some(0)),
                patch_node(rng, right, bbox, Some(0)),
            ],
            1 + rng.below(2) as usize,
        )
    } else {
        let dom = if rng.chance(0.5) {
            nat
        } else {
            sub_domain(rng, nat)
        };
        (vec![patch_node(rng, dom, bbox, None)], 0)
    };
    let patch_dom = nodes.get(node).map_or(nat, |n| n.dom);
    let dom = match rng.below(8) {
        0..=2 => nat,
        3 => sub_domain(rng, nat),
        // disjoint from the patch (to the right)
        4 => [
            patch_dom[2] + 0.1 * (nat[2] - nat[0]),
            nat[1],
            patch_dom[2] + 0.5 * (nat[2] - nat[0]),
            nat[3],
        ],
        // a sliver along the patch's edge
        5 => [patch_dom[0], patch_dom[1], patch_dom[0], patch_dom[3]],
        // touching the patch's edge
        6 => [patch_dom[2], nat[1], patch_dom[2] + 1.0, nat[3]],
        _ => [
            patch_dom[0] - 1.0,
            patch_dom[1] - 1.0,
            patch_dom[2] + 1.0,
            patch_dom[3] + 1.0,
        ],
    };
    let center = [
        0.5 * (bbox[0] + bbox[3]),
        0.5 * (bbox[1] + bbox[4]),
        0.5 * (bbox[2] + bbox[5]),
    ];
    let size = ((bbox[3] - bbox[0]).powi(2) + (bbox[4] - bbox[1]).powi(2) + (bbox[5] - bbox[2]).powi(2))
        .sqrt()
        .max(1e-12);
    let dir = unit_dir(rng);
    let (point, vector) = match rng.below(6) {
        // through the box centre region
        0 | 1 => {
            let off = rng.point3(-0.3, 0.3);
            (
                [
                    center[0] + off[0] * size,
                    center[1] + off[1] * size,
                    center[2] + off[2] * size,
                ],
                dir,
            )
        }
        // far off to the side, parallel to an offset direction: misses
        2 => {
            let side = unitize(cross(dir, unit_dir(rng)));
            (
                [
                    center[0] + side[0] * size * 3.0,
                    center[1] + side[1] * size * 3.0,
                    center[2] + side[2] * size * 3.0,
                ],
                dir,
            )
        }
        // starting beyond the box, pointing away (rays behind)
        3 => (
            [
                center[0] + dir[0] * size * 3.0,
                center[1] + dir[1] * size * 3.0,
                center[2] + dir[2] * size * 3.0,
            ],
            dir,
        ),
        // grazing the box near a corner
        4 => {
            let corners = box_corners(bbox);
            let c = pick(rng, &corners, [0.0; 3]);
            let side = unitize(cross(dir, unit_dir(rng)));
            let eps = pick(rng, &[0.0, 1e-9, 1e-6, 1e-3], 0.0) * size;
            (
                [c[0] + side[0] * eps, c[1] + side[1] * eps, c[2] + side[2] * eps],
                dir,
            )
        }
        // vertical through a random point of the box footprint, scaled vector
        _ => {
            let s = pick(rng, &[1.0, 0.01, 100.0], 1.0);
            (
                [
                    rng.uniform(bbox[0], bbox[3]),
                    rng.uniform(bbox[1], bbox[4]),
                    bbox[5] + size,
                ],
                [0.0, 0.0, -s],
            )
        }
    };
    let ivl = match rng.below(5) {
        0 | 1 => None,
        2 => Some([-size * 10.0, size * 10.0]),
        3 => Some([size * 20.0, size * 30.0]),
        _ => {
            let a = rng.uniform(-size * 2.0, size * 2.0);
            Some([a, a + rng.uniform(0.0, size * 2.0)])
        }
    };
    Inputs {
        surface,
        op: OpIn::Lsi {
            flags,
            nodes,
            node,
            dom,
            point,
            vector,
            ivl,
            ray: rng.chance(0.4),
            dist_tol: pick(rng, &[0.0, 1e-6, 1e-3], 1e-6) * scale,
            tol3d: pick(rng, &[1e-9, 1e-6, 1e-3], 1e-6) * scale,
        },
    }
}

/// A 3D B-spline curve over `[0, 1]` across the unit square at height `z`.
fn gen_curve(rng: &mut Rng, scale: f64) -> CurveData {
    let p = 1 + rng.below(3) as usize;
    let n1 = p + 1 + rng.below(3) as usize;
    let y0 = rng.uniform(0.0, 1.0);
    let y1 = rng.uniform(0.0, 1.0);
    let bend = rng.uniform(-0.3, 0.3);
    let cpts = (0..n1)
        .map(|i| {
            let t = i as f64 / (n1 - 1) as f64;
            let y = y0 + (y1 - y0) * t + if i > 0 && i + 1 < n1 { bend } else { 0.0 };
            [t * scale, y * scale, 0.0, NOW]
        })
        .collect();
    CurveData {
        p,
        cpts,
        knots: uniform_knots(n1, p),
    }
}

fn gen_combine(rng: &mut Rng) -> Inputs {
    let scale = pick(rng, &[1.0, 1.0, 1.0, 1e-3, 1e3], 1.0);
    let (surface, nat, bbox) = if rng.chance(0.5) {
        (
            SurfIn::Plane {
                o: [0.0, 0.0, 0.0],
                x: [1.0, 0.0, 0.0],
                y: [0.0, 1.0, 0.0],
                scale: [1.0, 1.0],
                domain: [0.0, 0.0, 1.0, 1.0],
            },
            [0.0, 0.0, 1.0, 1.0],
            [0.0, 0.0, 0.0, 1.0, 1.0, 0.0],
        )
    } else {
        gen_surface(rng, 1.0)
    };
    let curve = gen_curve(rng, scale);
    let ivl = if rng.chance(0.8) {
        [0.0, 1.0]
    } else {
        let a = rng.uniform(0.0, 0.3);
        [a, rng.uniform(a + 0.3, 1.0)]
    };
    let tol = pick(rng, &[1e-6, 1e-3, 1e-2], 1e-6) * scale;
    // surface tree: a head node and two patches
    let um = 0.5;
    let mut head = patch_node(rng, nat, bbox, None);
    head.aux = pick(rng, &[4, 6], 4);
    head.c1 = Some(1);
    head.c2 = Some(2);
    let mut left = patch_node(rng, [nat[0], nat[1], um, nat[3]], bbox, Some(0));
    left.aux = pick(rng, &[3, 3, 0, 5], 3);
    let right = patch_node(rng, [um, nat[1], nat[2], nat[3]], bbox, Some(0));
    let nodes = vec![head, left, right];
    let mut curve_nodes = Vec::new();
    let mut sols = Vec::new();
    let v_base = rng.uniform(0.05, 0.95);
    let uv_of = |t: f64, v: f64| [t, v];
    let count = 1 + rng.below(7) as usize;
    let mut t = ivl[0] + rng.uniform(0.0, 0.2) * (ivl[1] - ivl[0]);
    // points advance on their own; ranges continue from the last range end
    let mut tp = t;
    let first_point = rng.chance(0.1);
    let mut prev_point = false;
    for k in 0..count {
        let is_point = if k == 0 { first_point } else { rng.chance(0.35) };
        let dev = if rng.chance(0.85) {
            0.0
        } else {
            pick(rng, &[1e-17, 1e-9, 1e-6, 1e-3], 0.0) * scale
        };
        let mut v = v_base;
        if rng.chance(0.08) {
            // jump across the seam
            v = if v_base < 0.5 { v_base + 0.95 } else { v_base - 0.95 };
            v = v.clamp(nat[1], nat[3]);
        }
        if is_point {
            // close to the previous point (combined), or further on
            let step = if prev_point && rng.chance(0.75) {
                pick(rng, &[0.0, 1e-12, 1e-8], 0.0)
            } else {
                rng.uniform(0.0, 0.05)
            };
            tp = (tp + step).min(ivl[1]);
            let t = tp;
            let (n0, n1) = if rng.chance(0.4) {
                (None, None)
            } else {
                curve_nodes.push(CurveNodeIn {
                    aux: pick(rng, &[1, 2, 2, 0], 2),
                    ivl: [(t - 0.05).max(0.0), (t + 0.05).min(1.0)],
                });
                (Some(curve_nodes.len() - 1), Some(1 + rng.below(2) as usize))
            };
            let uv = uv_of(t, v);
            sols.push(SolIn {
                kind: 1,
                nvars: 3,
                sv: dev,
                s: [t, uv[0], uv[1]],
                ev: 0.0,
                e: [0.0, 0.0, 0.0],
                n0,
                n1,
            });
            prev_point = true;
        } else {
            let start = match rng.below(20) {
                // overlapping the previous range
                0..=3 => (t - 0.002).max(ivl[0]),
                // a gap
                4..=6 => (t + rng.uniform(0.0, 0.05)).min(ivl[1]),
                // across the closed curve's ends
                7 | 8 => (ivl[1] - 0.012).max(t),
                // touching the previous end
                _ => t,
            };
            let len = match rng.below(20) {
                0 | 1 => pick(rng, &[0.0, 1e-14, 1e-11], 0.0),
                2 => rng.uniform(0.03, 0.3),
                _ => rng.uniform(0.0005, 0.015),
            };
            let end = (start + len).min(ivl[1]);
            let (a, b) = (uv_of(start, v), uv_of(end, v));
            let ev = if rng.chance(0.85) { dev } else { dev * 1e6 };
            sols.push(SolIn {
                kind: 2,
                nvars: 3,
                sv: dev,
                s: [start, a[0], a[1]],
                ev,
                e: [end, b[0], b[1]],
                n0: None,
                n1: None,
            });
            t = end;
            tp = end;
            prev_point = false;
        }
    }
    if rng.chance(0.05) {
        sols.clear();
    }
    Inputs {
        surface,
        op: OpIn::Combine {
            curve,
            dom: nat,
            ivl,
            tol,
            curve_nodes,
            nodes,
            sols,
        },
    }
}

fn gen_inputs(rng: &mut Rng) -> Inputs {
    if rng.chance(0.5) {
        gen_lsi(rng)
    } else {
        gen_combine(rng)
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surface_p10"
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
