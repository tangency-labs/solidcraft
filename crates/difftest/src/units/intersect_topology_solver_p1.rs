//! Runner for unit `intersect.topology_solver_p1`: `SmTopologySolver` and its local solvers.
//!
//! C++: `solidcore_ffi::units::intersect_topology_solver_p1` (upstream
//! `source/SMLib/src/SmTopologySolver.cpp`). Rust: `solidcore::intersect::topology_solver::p1`.
//!
//! A case is one `op`: `ctor` (the constructor's operation category and projection flag,
//! `GetBestAnswerSoFar`), `may` (`MayContainAnswer` on box pairs), `local` (`SolveIt` of a
//! local solver called on pairs of objects, the solutions accumulating in the solver) or
//! `trees` (`SolveTrees` / `SolveTwoTrees`). Every case has the solver header `solver`
//! (`op` code, `req`, `tol`, `best` = `dBestAnswerSoFarSq`, optional `at` distance and
//! option vectors `vecs`). The `local` and `trees` cases have an object table `objects` of
//! free-standing vertices (`{"vertex": {p, tol}}`), curves (`{"curve": ..}`: `bspline`
//! (control points as stored `[x, y, z, w]`), `hermite`, `line`, `ellipse` or `circle`, as
//! unit `curve.curve_p5`) and edges (`{"edge": {curve, ivl}}`, owning the curve), and the
//! local solver `local` (`kind` as `solidcore_ffi::units::intersect_topology_solver_p1::
//! LocalKind`, `swap`, `point`, point vertex `pv`, interval `ivl`, curve `owner`, `curve`,
//! curve edge `cedge`; indices into the table, `-1` for none). On the Rust side the table
//! is a [`TopologyObjects`] over the same free-standing objects (vertex point and tolerance,
//! the edge's curve and interval, the curve's edge), the handles carrying the curves'
//! natural intervals.
//!
//! Both sides produce the flat record documented in the ffi module; the runner reports
//! its first entry as the `status` int (`0` = `SM_SUCCESS`, else the `SmStatus`; `1007`
//! for a Rust `InvalidInput`, `-1` for a Rust error upstream cannot produce) and the rest
//! as the `out` values. Solution objects are table indices, `-1` for `NULL`, `-2` for
//! another object (an analytic replacement made by a curve cache).
//!
//! Deferred cases: curve/curve solves other than intersections (`SmCurve::GlobalCurveSolve`,
//! unit `curve.curve_p9`), tree solves that reach `SmTopologySolver::LocalSolve` (unit
//! `intersect.topology_solver_p2`), solution comparisons that need an edge's parameter
//! interval (`topology.edge`) and the unported paths of the curve point solves.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::Curve;
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::global_solver::{SolutionRequested, SolverOperation};
use solidcore::geom::object::ShapeMember;
use solidcore::geom::solution_array::{ObjectKind, Solution, SolutionObject, SolutionType};
use solidcore::geom::tree::{NodeGeomType, Tree};
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::topology_solver::p1::{
    CurveRef, LocalSolver, LocalSolverKind, TopologyObjects, TopologySolver,
};
use solidcore::kernel::Kernel;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::math::NOW;
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::surface::surface::p2::Surface;
use solidcore::SolidError;
use solidcore_ffi::units::intersect_topology_solver_p1::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `intersect.topology_solver_p1` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

const BIG_DOUBLE: f64 = 1.0e20;

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

fn minus_one() -> i64 {
    -1
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
struct CircleIn {
    c: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    domain: [f64; 2],
    radius: f64,
    #[serde(default)]
    inside_out: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CurveIn {
    Bspline(BSplineIn),
    Hermite(HermiteIn),
    Line(LineIn),
    Ellipse(EllipseIn),
    Circle(CircleIn),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ObjIn {
    Vertex { p: [f64; 3], tol: f64 },
    Curve(CurveIn),
    Edge { curve: usize, ivl: [f64; 2] },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SolverIn {
    op: i32,
    #[serde(default = "req_all")]
    req: i32,
    tol: f64,
    #[serde(default = "big")]
    best: f64,
    #[serde(default)]
    at: Option<f64>,
    #[serde(default)]
    vecs: Vec<[f64; 3]>,
}

fn req_all() -> i32 {
    1
}

fn big() -> f64 {
    BIG_DOUBLE
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocalIn {
    kind: usize,
    #[serde(default)]
    swap: bool,
    #[serde(default)]
    point: [f64; 3],
    #[serde(default = "minus_one")]
    pv: i64,
    #[serde(default)]
    ivl: Option<[f64; 2]>,
    #[serde(default = "minus_one")]
    owner: i64,
    #[serde(default = "minus_one")]
    curve: i64,
    #[serde(default = "minus_one")]
    cedge: i64,
}

/// A box `[minx, miny, minz, maxx, maxy, maxz]`.
type BoxIn = [f64; 6];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TreeIn {
    Point { p: [f64; 3], size: f64 },
    Spatial { bbox: BoxIn, items: Vec<(i64, BoxIn)> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Ctor {
        solver: SolverIn,
    },
    May {
        solver: SolverIn,
        pairs: Vec<[BoxIn; 2]>,
    },
    Local {
        solver: SolverIn,
        objects: Vec<ObjIn>,
        local: LocalIn,
        calls: Vec<[i64; 2]>,
    },
    Trees {
        solver: SolverIn,
        objects: Vec<ObjIn>,
        local: LocalIn,
        tree1: TreeIn,
        tree2: TreeIn,
        #[serde(default)]
        two: bool,
    },
}

// ---- C++ input --------------------------------------------------------------------

fn b(x: bool) -> f64 {
    f64::from(u8::from(x))
}

fn curve_block(v: &mut Vec<f64>, c: &CurveIn) {
    match c {
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
            v.extend_from_slice(&l.p);
            v.extend_from_slice(&l.v);
            v.extend([l.domain[0], l.domain[1], l.scale, b(l.inside_out)]);
        }
        CurveIn::Ellipse(e) => {
            v.extend([3.0, 3.0]);
            for p in [e.c, e.x, e.y] {
                v.extend_from_slice(&p);
            }
            v.extend([e.domain[0], e.domain[1], e.radii[0], e.radii[1], b(e.inside_out)]);
        }
        CurveIn::Circle(e) => {
            v.extend([4.0, 3.0]);
            for p in [e.c, e.x, e.y] {
                v.extend_from_slice(&p);
            }
            v.extend([e.domain[0], e.domain[1], e.radius, b(e.inside_out)]);
        }
    }
}

fn header(v: &mut Vec<f64>, s: &SolverIn) {
    v.extend([
        f64::from(s.op),
        f64::from(s.req),
        s.tol,
        s.best,
        b(s.at.is_some()),
        s.at.unwrap_or(0.0),
        s.vecs.len() as f64,
    ]);
    for w in &s.vecs {
        v.extend_from_slice(w);
    }
}

fn objects_block(v: &mut Vec<f64>, objs: &[ObjIn]) {
    v.push(objs.len() as f64);
    for o in objs {
        match o {
            ObjIn::Vertex { p, tol } => {
                v.push(0.0);
                v.extend_from_slice(p);
                v.push(*tol);
            }
            ObjIn::Curve(c) => {
                v.push(1.0);
                curve_block(v, c);
            }
            ObjIn::Edge { curve, ivl } => {
                v.extend([2.0, *curve as f64, ivl[0], ivl[1]]);
            }
        }
    }
}

fn local_block(v: &mut Vec<f64>, l: &LocalIn) {
    v.extend([l.kind as f64, b(l.swap)]);
    v.extend_from_slice(&l.point);
    let ivl = l.ivl.unwrap_or([0.0, 0.0]);
    v.extend([
        l.pv as f64,
        b(l.ivl.is_some()),
        ivl[0],
        ivl[1],
        l.owner as f64,
        l.curve as f64,
        l.cedge as f64,
    ]);
}

fn tree_block(v: &mut Vec<f64>, t: &TreeIn) {
    match t {
        TreeIn::Point { p, size } => {
            v.push(0.0);
            v.extend_from_slice(p);
            v.push(*size);
        }
        TreeIn::Spatial { bbox, items } => {
            v.push(1.0);
            v.extend_from_slice(bbox);
            v.push(items.len() as f64);
            for (o, bx) in items {
                v.push(*o as f64);
                v.extend_from_slice(bx);
            }
        }
    }
}

fn cpp_input(op: &OpIn) -> (Op, Vec<f64>) {
    let mut v = Vec::new();
    match op {
        OpIn::Ctor { solver } => {
            header(&mut v, solver);
            (Op::Ctor, v)
        }
        OpIn::May { solver, pairs } => {
            header(&mut v, solver);
            v.push(pairs.len() as f64);
            for [b1, b2] in pairs {
                v.extend_from_slice(b1);
                v.extend_from_slice(b2);
            }
            (Op::May, v)
        }
        OpIn::Local {
            solver,
            objects,
            local,
            calls,
        } => {
            header(&mut v, solver);
            objects_block(&mut v, objects);
            local_block(&mut v, local);
            v.push(calls.len() as f64);
            for [o1, o2] in calls {
                v.extend([*o1 as f64, *o2 as f64]);
            }
            (Op::Local, v)
        }
        OpIn::Trees {
            solver,
            objects,
            local,
            tree1,
            tree2,
            two,
        } => {
            header(&mut v, solver);
            objects_block(&mut v, objects);
            local_block(&mut v, local);
            tree_block(&mut v, tree1);
            tree_block(&mut v, tree2);
            v.push(b(*two));
            (Op::Trees, v)
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
    let (code, input) = cpp_input(op);
    match ffi::eval(code, &input) {
        Ok(flat) => Ok(to_output(&flat)),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- Rust side --------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

fn ext(bx: &BoxIn) -> Extent3d {
    Extent3d::from_points(
        Vec3::new(bx[0], bx[1], bx[2]),
        Vec3::new(bx[3], bx[4], bx[5]),
    )
}

fn build(k: &Kernel, c: &CurveIn) -> Result<Curve, HarnessError> {
    match c {
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
        CurveIn::Line(l) => {
            let opts = BoundedLineOptions {
                scale: l.scale,
                inside_out: l.inside_out,
                ..BoundedLineOptions::default()
            };
            Ok(Curve::from(Line::bounded(
                v3(l.p),
                v3(l.v),
                &Extent1d::from_min_max(l.domain[0], l.domain[1]),
                &opts,
            )))
        }
        CurveIn::Ellipse(e) => {
            let opts = EllipseOptions {
                dim: 3,
                nurb: None,
                inside_out: e.inside_out,
            };
            Ok(Curve::from(
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
                .map_err(|e| harness(format!("ellipse: {e}")))?,
            ))
        }
        CurveIn::Circle(e) => {
            let opts = EllipseOptions {
                dim: 3,
                nurb: None,
                inside_out: e.inside_out,
            };
            Ok(Curve::from(
                Ellipse::new_circle(
                    k,
                    v3(e.c),
                    v3(e.x),
                    v3(e.y),
                    Extent1d::from_min_max(e.domain[0], e.domain[1]),
                    e.radius,
                    &opts,
                )
                .map_err(|e| harness(format!("circle: {e}")))?,
            ))
        }
    }
}

/// A handle to a table object (its index); a curve carries its natural interval.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Obj {
    Vertex(usize),
    Curve(usize, Extent1d),
    Edge(usize),
    /// The analytic replacement a curve cache makes of curve `i`.
    Analytic(usize),
}

impl SolutionObject for Obj {
    fn kind(&self) -> ObjectKind {
        match self {
            Obj::Vertex(_) => ObjectKind::Vertex,
            Obj::Curve(..) | Obj::Analytic(_) => ObjectKind::Curve,
            Obj::Edge(_) => ObjectKind::Edge,
        }
    }

    fn natural_interval(&self) -> Option<Extent1d> {
        match self {
            Obj::Curve(_, i) => Some(*i),
            _ => None,
        }
    }
}

enum Item {
    Vertex(Point3, f64),
    /// A curve and the edge owning it.
    Curve(Box<Curve>, Option<usize>),
    /// An edge's curve and interval.
    Edge(usize, Extent1d),
}

/// The free-standing objects of a case.
struct Scene {
    items: Vec<Item>,
}

fn not_found(what: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: "difftest::intersect_topology_solver_p1::Scene",
        reason: what,
    }
}

impl Scene {
    fn new(k: &Kernel, objs: &[ObjIn]) -> Result<Self, HarnessError> {
        let mut items: Vec<Item> = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            match o {
                ObjIn::Vertex { p, tol } => items.push(Item::Vertex(v3(*p), *tol)),
                ObjIn::Curve(c) => items.push(Item::Curve(Box::new(build(k, c)?), None)),
                ObjIn::Edge { curve, ivl } => {
                    let Some(Item::Curve(_, owner)) = items.get_mut(*curve) else {
                        return Err(harness("edge curve is not a curve"));
                    };
                    *owner = Some(i);
                    items.push(Item::Edge(*curve, Extent1d::from_min_max(ivl[0], ivl[1])));
                }
            }
        }
        Ok(Self { items })
    }

    /// The handle of object `i` (`None` for `-1`).
    fn handle(&self, i: i64) -> Result<Option<Obj>, HarnessError> {
        if i < 0 {
            return Ok(None);
        }
        let i = i as usize;
        match self.items.get(i) {
            Some(Item::Vertex(..)) => Ok(Some(Obj::Vertex(i))),
            Some(Item::Curve(c, _)) => Ok(Some(Obj::Curve(i, c.natural_interval()))),
            Some(Item::Edge(..)) => Ok(Some(Obj::Edge(i))),
            None => Err(harness("object index out of range")),
        }
    }

    fn curve_ref(&self, i: i64) -> Result<Option<CurveRef<'_, Obj>>, HarnessError> {
        match self.handle(i)? {
            Some(h @ Obj::Curve(j, _)) => match self.items.get(j) {
                Some(Item::Curve(c, _)) => Ok(Some(CurveRef { curve: c, handle: h })),
                _ => Err(harness("not a curve")),
            },
            None => Ok(None),
            Some(_) => Err(harness("not a curve")),
        }
    }
}

impl TopologyObjects<Obj> for Scene {
    fn curve(&self, object: Obj) -> solidcore::Result<&Curve> {
        match object {
            Obj::Curve(i, _) => match self.items.get(i) {
                Some(Item::Curve(c, _)) => Ok(c),
                _ => Err(not_found("not a curve")),
            },
            _ => Err(not_found("not a curve")),
        }
    }

    fn analytic_curve_handle(&self, curve: Obj) -> Option<Obj> {
        match curve {
            Obj::Curve(i, _) => Some(Obj::Analytic(i)),
            _ => None,
        }
    }

    fn vertex_point(&self, vertex: Obj) -> solidcore::Result<Point3> {
        match vertex {
            Obj::Vertex(i) => match self.items.get(i) {
                Some(Item::Vertex(p, _)) => Ok(*p),
                _ => Err(not_found("not a vertex")),
            },
            _ => Err(not_found("not a vertex")),
        }
    }

    fn vertex_tolerance(&self, vertex: Obj) -> solidcore::Result<f64> {
        match vertex {
            Obj::Vertex(i) => match self.items.get(i) {
                Some(Item::Vertex(_, t)) => Ok(*t),
                _ => Err(not_found("not a vertex")),
            },
            _ => Err(not_found("not a vertex")),
        }
    }

    fn curve_edge(&self, curve: Obj) -> solidcore::Result<Option<Obj>> {
        match curve {
            Obj::Curve(i, _) => match self.items.get(i) {
                Some(Item::Curve(_, owner)) => Ok(owner.map(Obj::Edge)),
                _ => Err(not_found("not a curve")),
            },
            _ => Ok(None),
        }
    }

    fn edge_curve(&self, edge: Obj) -> solidcore::Result<Option<Obj>> {
        match edge {
            Obj::Edge(i) => match self.items.get(i) {
                Some(Item::Edge(c, _)) => match self.items.get(*c) {
                    Some(Item::Curve(curve, _)) => {
                        Ok(Some(Obj::Curve(*c, curve.natural_interval())))
                    }
                    _ => Err(not_found("edge curve")),
                },
                _ => Err(not_found("not an edge")),
            },
            _ => Err(not_found("not an edge")),
        }
    }

    fn edge_interval(&self, edge: Obj) -> solidcore::Result<Extent1d> {
        match edge {
            Obj::Edge(i) => match self.items.get(i) {
                Some(Item::Edge(_, ivl)) => Ok(*ivl),
                _ => Err(not_found("not an edge")),
            },
            _ => Err(not_found("not an edge")),
        }
    }

    fn edge_tolerance(&self, _edge: Obj) -> solidcore::Result<f64> {
        // only the face solves read it, and the scene has no faces
        Err(not_found("no edge tolerances in the scene"))
    }

    fn surface_face(&self, _surface: Obj) -> solidcore::Result<Option<Obj>> {
        Err(not_found("no surfaces in the scene"))
    }

    fn face_surface(&self, _face: Obj) -> solidcore::Result<Obj> {
        Err(not_found("no faces in the scene"))
    }

    fn surface(&self, _surface: Obj) -> solidcore::Result<&Surface> {
        Err(not_found("no surfaces in the scene"))
    }

    fn face_uv_domain(&self, _face: Obj) -> solidcore::Result<Extent2d> {
        Err(not_found("no faces in the scene"))
    }

    fn face_tolerance(&self, _face: Obj) -> solidcore::Result<f64> {
        Err(not_found("no faces in the scene"))
    }

    fn surface_owner(&self, _surface: Obj) -> solidcore::Result<Option<Obj>> {
        Err(not_found("no surfaces in the scene"))
    }

    fn shape_member(&self, _object: Obj) -> ShapeMember {
        ShapeMember::Other
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

fn shim_id(o: Option<Obj>) -> f64 {
    match o {
        None => -1.0,
        Some(Obj::Vertex(i) | Obj::Curve(i, _) | Obj::Edge(i)) => i as f64,
        Some(Obj::Analytic(_)) => -2.0,
    }
}

/// A solution record (see the ffi module docs).
fn put_sol(o: &mut Vec<f64>, s: &Solution<Obj>) {
    o.extend([
        f64::from(s.solution_type as i32),
        s.num_variables as f64,
        s.num_objects as f64,
        if s.num_objects > 0 {
            shim_id(s.objects[0])
        } else {
            -1.0
        },
        if s.num_objects > 1 {
            shim_id(s.objects[1])
        } else {
            -1.0
        },
        s.start.solution_value,
    ]);
    let n = s.num_variables.min(10);
    o.extend(s.start.parameters.iter().take(n));
    if s.solution_type == SolutionType::RangeOfValues {
        o.push(s.end.solution_value);
        o.extend(s.end.parameters.iter().take(n));
    }
}

fn operation(code: i32) -> Result<SolverOperation, HarnessError> {
    SolverOperation::from_code(code).ok_or_else(|| harness("bad operation"))
}

fn requested(code: i32) -> Result<SolutionRequested, HarnessError> {
    SolutionRequested::from_code(code).ok_or_else(|| harness("bad request"))
}

fn local_kind(code: usize) -> Result<LocalSolverKind, HarnessError> {
    use LocalSolverKind as K;
    Ok(match code {
        0 => K::Base,
        1 => K::Point,
        2 => K::PointVertex,
        3 => K::VertexVertex,
        4 => K::PointCurve,
        5 => K::PointEdge,
        6 => K::VertexEdge,
        7 => K::Curve,
        8 => K::CurveEdge,
        9 => K::EdgeEdge,
        10 => K::CurveVertex,
        _ => return Err(harness("bad local solver kind")),
    })
}

fn new_solver<'a>(
    k: &'a Kernel,
    scene: &'a Scene,
    s: &SolverIn,
) -> Result<TopologySolver<'a, 'a, Obj>, HarnessError> {
    let vecs: Vec<Vec3> = s.vecs.iter().map(|v| v3(*v)).collect();
    let mut ts = TopologySolver::new(
        k,
        scene,
        operation(s.op)?,
        requested(s.req)?,
        s.tol,
        s.best,
        &vecs,
    );
    if let Some(at) = s.at {
        ts.base.set_at_distance(at);
    }
    Ok(ts)
}

/// The local solver of `l` over `scene` (as the shim sets it up).
fn new_local<'a>(scene: &'a Scene, l: &LocalIn) -> Result<LocalSolver<'a, Obj>, HarnessError> {
    let kind = local_kind(l.kind)?;
    let mut ls = LocalSolver::new(kind, l.swap, None);
    match l.kind {
        1..=6 => {
            ls.set_point(v3(l.point));
            if l.pv >= 0 {
                ls.set_point_vertex(scene.handle(l.pv)?);
            }
            if (4..=6).contains(&l.kind) {
                if let Some(i) = l.ivl {
                    ls.interval = Extent1d::from_min_max(i[0], i[1]);
                }
                if l.owner >= 0 {
                    ls.curve_owner = scene.handle(l.owner)?;
                }
            }
        }
        7..=10 => {
            let ivl = l
                .ivl
                .map_or_else(Extent1d::new, |i| Extent1d::from_min_max(i[0], i[1]));
            ls.set_curve(scene.curve_ref(l.curve)?, ivl);
            if l.cedge >= 0 {
                ls.set_curve_edge(scene.handle(l.cedge)?);
            }
        }
        _ => {}
    }
    Ok(ls)
}

fn put_result(o: &mut Vec<f64>, ts: &TopologySolver<'_, '_, Obj>) {
    o.push(ts.base.best_answer_so_far_sq);
    o.push(ts.base.solutions.len() as f64);
    for s in ts.base.solutions.as_slice() {
        put_sol(o, s);
    }
}

fn build_tree(scene: &Scene, t: &TreeIn) -> Result<Tree<Obj>, HarnessError> {
    match t {
        TreeIn::Point { p, size } => Ok(Tree::from_point(v3(*p), *size, true)),
        TreeIn::Spatial { bbox, items } => {
            let mut tree = Tree::new_spatial(&ext(bbox));
            for (o, bx) in items {
                tree.add_to_spatial_tree(&ext(bx), scene.handle(*o)?, NodeGeomType::Default)
                    .map_err(|e| harness(format!("tree: {e}")))?;
            }
            Ok(tree)
        }
    }
}

fn rust(op: &OpIn) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut o: Vec<f64> = Vec::new();
    match op {
        OpIn::Ctor { solver } => {
            let scene = Scene { items: Vec::new() };
            let ts = new_solver(&k, &scene, solver)?;
            o.push(0.0);
            o.push(f64::from(ts.base.category.code()));
            o.push(b(ts.base.projected_operation));
            let best = ts.best_answer_so_far();
            o.push(b(best.is_some()));
            o.push(best.unwrap_or(0.0));
        }
        OpIn::May { solver, pairs } => {
            let scene = Scene { items: Vec::new() };
            let ts = new_solver(&k, &scene, solver)?;
            o.push(0.0);
            for [b1, b2] in pairs {
                match ts.may_contain_answer(&ext(b1), &ext(b2)) {
                    Ok(f) => o.push(b(f)),
                    Err(e) => {
                        if let Some(d) = Output::unported(&e) {
                            return Ok(d);
                        }
                        return Ok(to_output(&[status(&e)]));
                    }
                }
            }
        }
        OpIn::Local {
            solver,
            objects,
            local,
            calls,
        } => {
            let scene = Scene::new(&k, objects)?;
            let mut ts = new_solver(&k, &scene, solver)?;
            let mut ls = new_local(&scene, local)?;
            let mut st = 0.0;
            let mut done = 0usize;
            for [o1, o2] in calls {
                let r = ls.solve_it(&k, &mut ts, scene.handle(*o1)?, scene.handle(*o2)?);
                done += 1;
                if let Err(e) = r {
                    if let Some(d) = Output::unported(&e) {
                        return Ok(d);
                    }
                    st = status(&e);
                    break;
                }
            }
            o.push(st);
            o.push(done as f64);
            put_result(&mut o, &ts);
        }
        OpIn::Trees {
            solver,
            objects,
            local,
            tree1,
            tree2,
            two,
        } => {
            let scene = Scene::new(&k, objects)?;
            let t1 = build_tree(&scene, tree1)?;
            let t2 = build_tree(&scene, tree2)?;
            let mut ts = new_solver(&k, &scene, solver)?;
            let mut ls = new_local(&scene, local)?;
            let r = if *two {
                ts.solve_two_trees(&t1, &t2, &mut ls)
            } else {
                ts.solve_trees(&t1, &t2, &mut ls)
            };
            let st = match r {
                Ok(()) => 0.0,
                Err(e) => {
                    if let Some(d) = Output::unported(&e) {
                        return Ok(d);
                    }
                    status(&e)
                }
            };
            o.push(st);
            o.push(1.0);
            put_result(&mut o, &ts);
        }
    }
    Ok(to_output(&o))
}

// ---- generated cases --------------------------------------------------------------

fn rp(rng: &mut Rng, s: f64) -> [f64; 3] {
    [rng.uniform(-s, s), rng.uniform(-s, s), rng.uniform(-s, s)]
}

fn add(a: [f64; 3], c: [f64; 3]) -> [f64; 3] {
    [a[0] + c[0], a[1] + c[1], a[2] + c[2]]
}

fn scale3(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn unit(v: [f64; 3]) -> [f64; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l < 1e-12 {
        [0.0, 0.0, 1.0]
    } else {
        [v[0] / l, v[1] / l, v[2] / l]
    }
}

fn gen_dir(rng: &mut Rng) -> [f64; 3] {
    match rng.below(5) {
        0 => [1.0, 0.0, 0.0],
        1 => [0.0, 0.0, 1.0],
        2 => [0.0, -1.0, 0.0],
        _ => unit(rp(rng, 1.0)),
    }
}

/// Two unit vectors: a projection normal and a direction in its plane (the directed
/// operations), sometimes not perpendicular.
fn gen_vecs(rng: &mut Rng, n: usize) -> Vec<[f64; 3]> {
    let v0 = gen_dir(rng);
    let mut v1 = gen_dir(rng);
    if rng.chance(0.7) {
        // a direction perpendicular to v0
        let d = v1[0] * v0[0] + v1[1] * v0[1] + v1[2] * v0[2];
        let p = add(v1, scale3(v0, -d));
        let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        v1 = if l < 1e-6 {
            unit([v0[1], -v0[0] + 0.3, v0[2] + 0.1])
        } else {
            unit(p)
        };
    }
    let mut out = vec![v0, v1, gen_dir(rng)];
    out.truncate(n);
    out
}

/// The option vector count an operation reads (as the ffi validation).
fn vectors_needed(op: i32) -> usize {
    match op {
        11..=13 | 21 => 2,
        8 | 9 | 14 | 15 | 20 => 1,
        _ => 0,
    }
}

fn gen_scale(rng: &mut Rng) -> f64 {
    *rng.choose(&[1.0, 1.0, 1.0, 1.0, 1e-3, 1e3, 1e5, 0.01])
        .unwrap_or(&1.0)
}

fn gen_tol(rng: &mut Rng, s: f64) -> f64 {
    s * *rng.choose(&[1e-6, 1e-6, 1e-4, 1e-3, 1e-8, 0.1, 0.0]).unwrap_or(&1e-6)
}

/// A best answer for distances around `d`: none, around `d`, or exactly `d * d`.
fn gen_best(rng: &mut Rng, d: f64, signed: bool) -> f64 {
    match rng.below(6) {
        0 | 1 => BIG_DOUBLE,
        2 => d * d,
        3 => {
            let f = rng.uniform(0.2, 2.0) * d;
            if signed && rng.chance(0.4) {
                -f * f
            } else {
                f * f
            }
        }
        4 => 0.0,
        _ => (d * rng.uniform(0.5, 1.5)).powi(2),
    }
}

fn gen_solver(rng: &mut Rng, ops: &[i32], s: f64, d: f64) -> SolverIn {
    let op = *rng.choose(ops).unwrap_or(&3);
    let signed = matches!(op, 9 | 11 | 21);
    let nv = vectors_needed(op).max(if rng.chance(0.3) { 3 } else { 0 });
    SolverIn {
        op,
        req: i32::from(!rng.chance(0.2)),
        tol: gen_tol(rng, s),
        best: gen_best(rng, d, signed),
        at: if op == 6 || rng.chance(0.1) {
            Some(d * rng.uniform(0.5, 1.5))
        } else {
            None
        },
        vecs: gen_vecs(rng, nv),
    }
}

fn gen_box(rng: &mut Rng, s: f64) -> BoxIn {
    let c = rp(rng, s);
    let h = [
        rng.uniform(0.0, 0.5 * s),
        rng.uniform(0.0, 0.5 * s),
        if rng.chance(0.2) {
            0.0
        } else {
            rng.uniform(0.0, 0.5 * s)
        },
    ];
    [
        c[0] - h[0],
        c[1] - h[1],
        c[2] - h[2],
        c[0] + h[0],
        c[1] + h[1],
        c[2] + h[2],
    ]
}

fn gen_line(rng: &mut Rng, s: f64) -> CurveIn {
    let p = rp(rng, s);
    let v = scale3(gen_dir(rng), rng.uniform(0.5, 2.0));
    let a = rng.uniform(-s, 0.0);
    CurveIn::Line(LineIn {
        p,
        v,
        domain: [a, a + rng.uniform(0.2, 2.0) * s],
        scale: 1.0,
        inside_out: rng.chance(0.1),
    })
}

fn gen_axes(rng: &mut Rng) -> ([f64; 3], [f64; 3]) {
    if rng.chance(0.5) {
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
    } else {
        let x = [1.0, rng.uniform(-0.5, 0.5), rng.uniform(-0.5, 0.5)];
        let y = [rng.uniform(-0.3, 0.3), 1.0, rng.uniform(-0.5, 0.5)];
        (x, y)
    }
}

fn gen_domain(rng: &mut Rng) -> [f64; 2] {
    let d0 = *rng.choose(&[0.0, 0.0, -90.0, 30.0]).unwrap_or(&0.0);
    if rng.chance(0.4) {
        [d0, d0 + 360.0]
    } else {
        [d0, d0 + rng.uniform(10.0, 359.0)]
    }
}

fn gen_bspline(rng: &mut Rng, s: f64) -> CurveIn {
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(4) as usize;
    let rational = rng.chance(0.25);
    let mut cpts = Vec::new();
    let mut at = rp(rng, s);
    for _ in 0..=n {
        let w = if rational { rng.uniform(0.5, 2.0) } else { NOW };
        let ww = if rational { w } else { 1.0 };
        cpts.push([at[0] * ww, at[1] * ww, at[2] * ww, w]);
        at = add(at, scale3(rp(rng, 1.0), 0.6 * s));
    }
    let interior = n - p;
    let mut u = vec![0.0; p + 1];
    let mut t = 0.0;
    for _ in 0..interior {
        t += rng.uniform(0.2, 1.0);
        u.push(t);
    }
    let end = t + rng.uniform(0.2, 1.0);
    u.extend(std::iter::repeat_n(end, p + 1));
    CurveIn::Bspline(BSplineIn {
        dim: 3,
        oob: false,
        cpts,
        p,
        u,
    })
}

fn gen_curve(rng: &mut Rng, s: f64) -> CurveIn {
    match rng.below(10) {
        0..=2 => gen_line(rng, s),
        3..=5 => gen_bspline(rng, s),
        6 => CurveIn::Hermite(HermiteIn {
            dim: 3,
            p1: rp(rng, s),
            d1: rp(rng, s),
            p2: rp(rng, s),
            d2: rp(rng, s),
        }),
        7 | 8 => {
            let (x, y) = gen_axes(rng);
            CurveIn::Circle(CircleIn {
                c: rp(rng, s),
                x,
                y,
                domain: gen_domain(rng),
                radius: s * rng.uniform(0.2, 1.0),
                inside_out: rng.chance(0.1),
            })
        }
        _ => {
            let (x, y) = gen_axes(rng);
            let r = s * rng.uniform(0.2, 1.0);
            CurveIn::Ellipse(EllipseIn {
                c: rp(rng, s),
                x,
                y,
                domain: gen_domain(rng),
                radii: [r, r * rng.uniform(0.2, 0.95)],
                inside_out: rng.chance(0.1),
            })
        }
    }
}

/// The parameter range of a generated curve.
fn range_of(c: &CurveIn) -> [f64; 2] {
    match c {
        CurveIn::Bspline(bs) => [
            bs.u.first().copied().unwrap_or(0.0),
            bs.u.last().copied().unwrap_or(1.0),
        ],
        CurveIn::Hermite(_) => [0.0, 1.0],
        CurveIn::Line(l) => [l.domain[0].min(l.domain[1]), l.domain[0].max(l.domain[1])],
        CurveIn::Ellipse(e) => e.domain,
        CurveIn::Circle(e) => e.domain,
    }
}

/// A sub-interval of `r` (or `r` itself).
fn gen_ivl(rng: &mut Rng, r: [f64; 2]) -> [f64; 2] {
    if rng.chance(0.5) {
        return r;
    }
    let len = r[1] - r[0];
    let a = r[0] + rng.uniform(0.0, 0.4) * len;
    let b = r[1] - rng.uniform(0.0, 0.4) * len;
    if b > a {
        [a, b]
    } else {
        r
    }
}

/// A point on `c` at a random parameter of its range (evaluated by the Rust curve,
/// bit-identical to upstream), else on a control point or centre.
fn on_curve(rng: &mut Rng, c: &CurveIn) -> [f64; 3] {
    let r = range_of(c);
    let t = match rng.below(5) {
        0 => r[0],
        1 => r[1],
        _ => r[0] + rng.unit() * (r[1] - r[0]),
    };
    let k = Kernel::default();
    if let Ok(p) = build(&k, c).and_then(|cv| {
        cv.evaluate_point(&k, t)
            .map_err(|e| harness(format!("evaluate: {e}")))
    }) {
        return [p.x, p.y, p.z];
    }
    match c {
        CurveIn::Bspline(bs) => bs.cpts.first().map_or([0.0; 3], |p| {
            if p[3] == NOW {
                [p[0], p[1], p[2]]
            } else {
                [p[0] / p[3], p[1] / p[3], p[2] / p[3]]
            }
        }),
        CurveIn::Hermite(h) => h.p1,
        CurveIn::Line(l) => l.p,
        CurveIn::Ellipse(e) => e.c,
        CurveIn::Circle(e) => e.c,
    }
}

/// A point near the curve: on it, within or just outside the tolerance `tol`, or
/// farther away (sometimes a conic's centre).
fn near_curve(rng: &mut Rng, c: &CurveIn, s: f64, tol: f64) -> [f64; 3] {
    if let (CurveIn::Circle(CircleIn { c: centre, .. }) | CurveIn::Ellipse(EllipseIn { c: centre, .. }), true) =
        (c, rng.chance(0.1))
    {
        return *centre;
    }
    let base = on_curve(rng, c);
    let off = match rng.below(6) {
        0 | 1 => [0.0; 3],
        2 => scale3(unit(rp(rng, 1.0)), 0.5 * tol),
        3 => scale3(unit(rp(rng, 1.0)), 2.0 * tol),
        _ => scale3(rp(rng, 1.0), s),
    };
    add(base, off)
}

const VERTEX_OPS: [i32; 25] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
];
const CURVE_OPS: [i32; 13] = [0, 3, 3, 3, 4, 5, 6, 8, 9, 11, 12, 13, 14];
const CC_OPS: [i32; 6] = [0, 0, 0, 0, 3, 4];

fn gen_vertex_case(rng: &mut Rng) -> OpIn {
    let s = gen_scale(rng);
    let nv = 1 + rng.below(5) as usize;
    let mut objects = Vec::new();
    let point = rp(rng, s);
    for _ in 0..nv {
        let p = match rng.below(4) {
            0 => point,
            1 => add(point, scale3(rp(rng, 1.0), 1e-9 * s)),
            _ => rp(rng, s),
        };
        let tol = if rng.chance(0.5) {
            0.0
        } else {
            gen_tol(rng, s)
        };
        objects.push(ObjIn::Vertex { p, tol });
    }
    let vv = rng.chance(0.3);
    let swap = rng.chance(0.3);
    let d = s;
    let mut solver = gen_solver(rng, &VERTEX_OPS, s, d);
    // aim rays at a vertex (sometimes off by about its tolerance)
    if matches!(solver.op, 8 | 9) && rng.chance(0.6) {
        let i = rng.below(nv as u64) as usize;
        if let (Some(ObjIn::Vertex { p, tol }), Some(v0)) = (objects.get(i), solver.vecs.first_mut()) {
            let off = scale3(unit(rp(rng, 1.0)), *rng.choose(&[0.0, 0.5, 1.0, 2.0]).unwrap_or(&0.0) * (*tol + solver.tol));
            let d = add(add(*p, off), scale3(point, -1.0));
            if d.iter().map(|x| x * x).sum::<f64>() > 1e-12 * s * s {
                *v0 = unit(d);
            }
        }
    }
    let mut calls = Vec::new();
    for _ in 0..=rng.below(4) {
        let a = rng.below(nv as u64) as i64;
        let c = rng.below(nv as u64) as i64;
        // vertex vertex: the point vertex comes first (second when swapped)
        let pair = if swap { [c, a] } else { [a, c] };
        let pair = if !vv && rng.chance(0.03) {
            // a NULL vertex (upstream's NER)
            if swap {
                [-1, c]
            } else {
                [c, -1]
            }
        } else {
            pair
        };
        calls.push(if vv && pair.contains(&-1) { [a, a] } else { pair });
    }
    let pv = if !vv && rng.chance(0.3) {
        rng.below(nv as u64) as i64
    } else {
        -1
    };
    OpIn::Local {
        solver,
        objects,
        local: LocalIn {
            kind: if vv { 3 } else { 2 },
            swap,
            point,
            pv,
            ivl: None,
            owner: -1,
            curve: -1,
            cedge: -1,
        },
        calls,
    }
}

fn gen_point_curve_case(rng: &mut Rng) -> OpIn {
    let s = gen_scale(rng);
    let nc = 1 + rng.below(3) as usize;
    let mut objects = Vec::new();
    let mut curves = Vec::new();
    for _ in 0..nc {
        let c = gen_curve(rng, s);
        curves.push(c.clone());
        objects.push(ObjIn::Curve(c));
    }
    // kind: 4 point/curve, 5 point/edge, 6 vertex/edge
    let kind = *rng.choose(&[4usize, 4, 5, 5, 6]).unwrap_or(&4);
    let mut edge_of = Vec::new();
    if kind != 4 || rng.chance(0.3) {
        for (i, c) in curves.iter().enumerate() {
            let ivl = gen_ivl(rng, range_of(c));
            edge_of.push(objects.len() as i64);
            objects.push(ObjIn::Edge { curve: i, ivl });
        }
    }
    let swap = rng.chance(0.3);
    let mut solver = gen_solver(rng, &CURVE_OPS, s, s);
    let point = match curves.first() {
        Some(c) => near_curve(rng, c, s, solver.tol),
        None => rp(rng, s),
    };
    // aim rays at the curve
    if matches!(solver.op, 8 | 9) && rng.chance(0.6) {
        if let (Some(c), Some(v0)) = (curves.first(), solver.vecs.first_mut()) {
            let target = on_curve(rng, c);
            let d = add(target, scale3(point, -1.0));
            if d.iter().map(|x| x * x).sum::<f64>() > 1e-12 * s * s {
                *v0 = unit(d);
            }
        }
    }
    let vtx = objects.len() as i64;
    objects.push(ObjIn::Vertex {
        p: point,
        tol: if rng.chance(0.5) { 0.0 } else { gen_tol(rng, s) },
    });
    let mut calls = Vec::new();
    let c0 = rng.below(nc as u64) as usize;
    let ivl = if kind == 4 {
        curves.get(c0).map(|c| gen_ivl(rng, range_of(c)))
    } else {
        None
    };
    for _ in 0..=rng.below(3) {
        let c = if kind == 4 { c0 } else { rng.below(nc as u64) as usize } as i64;
        let other = if kind == 6 { vtx } else { -1 };
        calls.push(if swap { [c, other] } else { [other, c] });
    }
    let pv = if kind != 6 && rng.chance(0.3) { vtx } else { -1 };
    let owner = if kind == 4 && !edge_of.is_empty() && rng.chance(0.5) {
        edge_of.get(c0).copied().unwrap_or(-1)
    } else {
        -1
    };
    OpIn::Local {
        solver,
        objects,
        local: LocalIn {
            kind,
            swap,
            point,
            pv,
            ivl,
            owner,
            curve: -1,
            cedge: -1,
        },
        calls,
    }
}

/// A curve crossing `c` near its parameter fraction `f`.
fn gen_crossing(rng: &mut Rng, c: &CurveIn, s: f64) -> CurveIn {
    let p = on_curve(rng, c);
    let d = gen_dir(rng);
    CurveIn::Line(LineIn {
        p: add(p, scale3(d, -s)),
        v: d,
        domain: [0.0, 2.0 * s],
        scale: 1.0,
        inside_out: false,
    })
}

fn gen_curve_curve_case(rng: &mut Rng) -> OpIn {
    let s = gen_scale(rng);
    let c1 = gen_curve(rng, s);
    let c2 = if rng.chance(0.6) {
        gen_crossing(rng, &c1, s)
    } else {
        gen_curve(rng, s)
    };
    let r1 = range_of(&c1);
    let r2 = range_of(&c2);
    let mut objects = vec![ObjIn::Curve(c1), ObjIn::Curve(c2)];
    objects.push(ObjIn::Edge {
        curve: 0,
        ivl: gen_ivl(rng, r1),
    });
    objects.push(ObjIn::Edge {
        curve: 1,
        ivl: gen_ivl(rng, r2),
    });
    let ee = rng.chance(0.5);
    let swap = rng.chance(0.3);
    let solver = gen_solver(rng, &CC_OPS, s, s);
    let local = if ee {
        LocalIn {
            kind: 9,
            swap,
            point: [0.0; 3],
            pv: -1,
            ivl: None,
            owner: -1,
            curve: -1,
            cedge: -1,
        }
    } else {
        LocalIn {
            kind: 8,
            swap,
            point: [0.0; 3],
            pv: -1,
            ivl: Some(gen_ivl(rng, r1)),
            owner: -1,
            curve: 0,
            cedge: if rng.chance(0.5) { 2 } else { -1 },
        }
    };
    let pair = if ee { [0, 1] } else { [-1, 1] };
    let pair = if swap { [pair[1], pair[0]] } else { pair };
    OpIn::Local {
        solver,
        objects,
        local,
        calls: vec![pair],
    }
}

fn gen_may_case(rng: &mut Rng) -> OpIn {
    let s = gen_scale(rng);
    let solver = gen_solver(rng, &VERTEX_OPS, s, s);
    let n = 1 + rng.below(12) as usize;
    let mut pairs = Vec::new();
    for _ in 0..n {
        let b1 = gen_box(rng, s);
        let b2 = if rng.chance(0.3) {
            // a nearby box
            let off = scale3(rp(rng, 1.0), 0.6 * s);
            let mut b2 = b1;
            for (v, o) in b2.iter_mut().zip(off.iter().cycle()) {
                *v += *o;
            }
            b2
        } else {
            gen_box(rng, s)
        };
        pairs.push([b1, b2]);
    }
    OpIn::May { solver, pairs }
}

fn gen_ctor_case(rng: &mut Rng) -> OpIn {
    let s = gen_scale(rng);
    OpIn::Ctor {
        solver: gen_solver(rng, &VERTEX_OPS, s, s),
    }
}

fn gen_abstract_case(rng: &mut Rng) -> OpIn {
    let s = gen_scale(rng);
    OpIn::Local {
        solver: gen_solver(rng, &[0, 3, 4], s, s),
        objects: vec![ObjIn::Vertex {
            p: rp(rng, s),
            tol: 0.0,
        }],
        local: LocalIn {
            kind: *rng.choose(&[0usize, 1, 7]).unwrap_or(&0),
            swap: rng.chance(0.5),
            point: rp(rng, s),
            pv: -1,
            ivl: None,
            owner: -1,
            curve: -1,
            cedge: -1,
        },
        calls: vec![[0, 0]],
    }
}

/// The box of a vertex grown by `g`.
fn point_box(p: [f64; 3], g: f64) -> BoxIn {
    [p[0] - g, p[1] - g, p[2] - g, p[0] + g, p[1] + g, p[2] + g]
}

fn gen_trees_case(rng: &mut Rng) -> OpIn {
    let s = gen_scale(rng);
    let nv = 1 + rng.below(4) as usize;
    let mut objects = Vec::new();
    let mut items = Vec::new();
    for i in 0..nv {
        let p = rp(rng, s);
        objects.push(ObjIn::Vertex { p, tol: 0.0 });
        items.push((i as i64, point_box(p, 0.0)));
    }
    let point = rp(rng, s);
    let solver = gen_solver(rng, &[0, 0, 3, 4], s, s);
    let far = rng.chance(0.5);
    let tree1 = TreeIn::Point {
        p: if far { scale3(point, 1e3) } else { point },
        size: 0.0,
    };
    OpIn::Trees {
        solver,
        objects,
        local: LocalIn {
            kind: 2,
            swap: false,
            point,
            pv: -1,
            ivl: None,
            owner: -1,
            curve: -1,
            cedge: -1,
        },
        tree1,
        tree2: TreeIn::Spatial {
            bbox: [-s, -s, -s, s, s, s],
            items,
        },
        two: rng.chance(0.5),
    }
}

fn gen_case(rng: &mut Rng) -> OpIn {
    match rng.below(100) {
        0..=3 => gen_ctor_case(rng),
        4..=17 => gen_may_case(rng),
        18..=19 => gen_abstract_case(rng),
        20..=49 => gen_vertex_case(rng),
        50..=79 => gen_point_curve_case(rng),
        80..=94 => gen_curve_curve_case(rng),
        _ => gen_trees_case(rng),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.topology_solver_p1"
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
        serde_json::to_value(gen_case(rng)).ok()
    }
}

/// Encodes a tree fixture for the p2 reference entry without duplicating the protocol.
pub(super) fn p2_tree_input(case: &Case) -> Result<Vec<f64>, HarnessError> {
    let parsed: OpIn = case.parse()?;
    if !matches!(parsed, OpIn::Trees { .. }) { return Err(harness("p2 tree fixture requires op=trees")); }
    Ok(cpp_input(&parsed).1)
}
/// Decodes the shared reference result for the p2 runner.
pub(super) fn p2_tree_output(flat: &[f64]) -> Output { to_output(flat) }
/// Generates a bounded tree fixture, seeded independently by the p2 unit id.
pub(super) fn p2_tree_generate(rng: &mut Rng) -> Option<Value> { serde_json::to_value(gen_trees_case(rng)).ok() }

/// Encodes a p2 local fixture using the shared object-construction protocol.
pub(super) fn p2_local_input(case: &Case) -> Result<Vec<f64>, HarnessError> {
    let parsed: OpIn = case.parse()?;
    if !matches!(parsed, OpIn::Local { .. }) { return Err(harness("p2 local fixture requires op=local")); }
    Ok(cpp_input(&parsed).1)
}
