//! Differential coverage of topology p2's local tree traversal.
//!
//! The shared p1 fixture adapter builds the same objects and trees on both sides;
//! the p2 C++ entry reaches the pinned `SmTopologySolver::LocalSolve`.

use super::intersect_topology_solver_p1 as shared;
use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};
use serde::Deserialize;
use serde_json::Value;
use solidcore::curve::curve::Curve;
use solidcore::error::{Result as SolidResult, SolidError};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::global_solver::{SolutionRequested, SolverOperation};
use solidcore::geom::object::{Shape, ShapeMember};
use solidcore::geom::solution_array::{ObjectKind, SolutionObject, SolutionType};
use solidcore::geom::tree::{NodeGeomType, Tree};
use solidcore::intersect::topology_solver::p1::{CurveRef, TopologyObjects, TopologySolver};
use solidcore::intersect::topology_solver::p2::{TopologySolveOptions, TopologyTrees};
use solidcore::kernel::Kernel;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::crv_basic::Curve as NlCurve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::surface::surface::Surface;
use solidcore_ffi::units::intersect_topology_solver_p2 as ffi;

/// Topology solver p2 differential runner.
pub struct Runner;

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.topology_solver_p2"
    }
    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        crate::runner::default_cases(
            self,
            &CaseContext {
                seeds: ctx.seeds.max(600),
                ..ctx.clone()
            },
        )
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        if case.inputs.get("op").and_then(Value::as_str) == Some("static") {
            return ffi::eval_static(&static_input(case)?)
                .map(|v| static_output(&v))
                .map_err(|e| HarnessError::new(e.to_string()));
        }
        let output = if case.inputs.get("op").and_then(Value::as_str) == Some("local") {
            ffi::eval_local(&shared::p2_local_input(case)?)
        } else {
            ffi::eval_trees(&shared::p2_tree_input(case)?)
        };
        output
            .map(|v| shared::p2_tree_output(&v))
            .map_err(|e| HarnessError::new(e.to_string()))
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        if case.inputs.get("op").and_then(Value::as_str) == Some("static") {
            return static_rust(case);
        }
        shared::Runner.run_rust(case)
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        shared::p2_tree_generate(rng)
    }
}

// Independent fixtures for the static entrypoints. The adapter supplies cache
// trees for real C++ shell-vertex B-reps; this does not port SmBrepCache itself.

#[derive(Deserialize)]
struct StaticInputs {
    #[serde(default)]
    swap: bool,
    #[serde(default)]
    included: bool,
    entry: i32,
    operation: i32,
    request: i32,
    tolerance: f64,
    point: [f64; 3],
    first: Vec<[f64; 3]>,
    second: Vec<[f64; 3]>,
}
fn static_best(operation: i32) -> f64 {
    if matches!(operation, 4 | 13 | 15) {
        0.0
    } else {
        1e30
    }
}
fn static_input(case: &Case) -> Result<Vec<f64>, HarnessError> {
    let i: StaticInputs = case.parse()?;
    let mut out = vec![
        f64::from(i.entry),
        f64::from(i.operation),
        f64::from(i.request),
        i.tolerance,
        static_best(i.operation),
        i.first.len() as f64,
        i.second.len() as f64,
    ];
    out.extend(i.point);
    for p in i.first.iter().chain(&i.second) {
        out.extend(p);
    }
    out.push(f64::from(u8::from(i.swap)));
    out.push(f64::from(u8::from(i.included)));
    Ok(out)
}
fn static_output(flat: &[f64]) -> Output {
    Output::new()
        .scalar("status", flat.first().copied().unwrap_or(-1.0))
        .values("static", flat.get(1..).unwrap_or_default().to_vec())
}
/// The shim's `id(object)` of a solution object: the vertex index, `999` for the
/// curve, `-2` for anything else (the faces and surfaces of face/face solves).
fn static_object_id(h: StaticHandle) -> f64 {
    if (0..=999).contains(&h.0) {
        f64::from(h.0)
    } else {
        -2.0
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
struct StaticHandle(i32);
impl SolutionObject for StaticHandle {
    fn kind(&self) -> ObjectKind {
        match self.0 {
            999 | -2 => ObjectKind::Curve,
            1000 | 1001 => ObjectKind::Other,
            2000 | 2001 => ObjectKind::Face,
            2002 | 2003 => ObjectKind::Surface,
            2004 => ObjectKind::Edge,
            _ => ObjectKind::Vertex,
        }
    }
    fn natural_interval(&self) -> Option<Extent1d> {
        (self.0 == 999 || self.0 == -2).then(|| Extent1d::from_min_max(0.0, 1.0))
    }
}
struct StaticScene {
    points: Vec<Point3>,
    first: Tree<StaticHandle>,
    second: Tree<StaticHandle>,
    empty_first: Tree<StaticHandle>,
    empty_second: Tree<StaticHandle>,
    curve: Curve,
    edge_owner: bool,
    surfaces: [Surface; 2],
}
fn static_error() -> SolidError {
    SolidError::invalid_input(
        "difftest topology p2 static adapter",
        "object absent from shell-vertex fixture",
    )
}
fn static_tree(
    points: &[Point3],
    offset: usize,
    tolerance: f64,
) -> SolidResult<Tree<StaticHandle>> {
    let mut bbox = Extent3d::new();
    for point in points {
        let mut b = Extent3d::from_point(*point);
        b.expand_absolute(tolerance);
        bbox = bbox.union(&b);
    }
    bbox.expand_absolute(tolerance * 100.0);
    let size = bbox.size();
    bbox.add_point(bbox.max() + size * 0.15);
    bbox.add_point(bbox.min() - size * 0.15);
    let mut tree = Tree::new_spatial(&bbox);
    for (index, point) in points.iter().enumerate() {
        let mut b = Extent3d::from_point(*point);
        b.expand_absolute(tolerance);
        tree.add_to_spatial_tree(
            &b,
            Some(StaticHandle((offset + index) as i32)),
            NodeGeomType::Default,
        )?;
    }
    Ok(tree)
}
impl TopologyObjects<StaticHandle> for StaticScene {
    fn brep_trees(&self, brep: StaticHandle) -> SolidResult<TopologyTrees<'_, StaticHandle>> {
        let (vertices, empty) = match brep.0 {
            1000 => (&self.first, &self.empty_first),
            1001 => (&self.second, &self.empty_second),
            _ => return Err(static_error()),
        };
        Ok(TopologyTrees {
            vertices,
            curves: empty,
            surfaces: empty,
        })
    }
    fn curve(&self, object: StaticHandle) -> SolidResult<&Curve> {
        if object.0 == 999 {
            Ok(&self.curve)
        } else {
            Err(static_error())
        }
    }
    fn analytic_curve_handle(&self, _: StaticHandle) -> Option<StaticHandle> {
        Some(StaticHandle(-2))
    }
    fn vertex_point(&self, vertex: StaticHandle) -> SolidResult<Point3> {
        usize::try_from(vertex.0)
            .ok()
            .and_then(|i| self.points.get(i))
            .copied()
            .ok_or_else(static_error)
    }
    fn vertex_tolerance(&self, vertex: StaticHandle) -> SolidResult<f64> {
        self.vertex_point(vertex)?;
        Ok(1e-5)
    }
    fn curve_edge(&self, _: StaticHandle) -> SolidResult<Option<StaticHandle>> {
        Ok(self.edge_owner.then_some(StaticHandle(2004)))
    }
    fn edge_curve(&self, edge: StaticHandle) -> SolidResult<Option<StaticHandle>> {
        if self.edge_owner && edge.0 == 2004 {
            Ok(Some(StaticHandle(999)))
        } else {
            Err(static_error())
        }
    }
    fn edge_interval(&self, edge: StaticHandle) -> SolidResult<Extent1d> {
        self.edge_curve(edge)?;
        Ok(Extent1d::from_min_max(0.0, 1.0))
    }
    fn edge_tolerance(&self, edge: StaticHandle) -> SolidResult<f64> {
        self.edge_curve(edge)?;
        Ok(1e-5)
    }
    fn surface_face(&self, surface: StaticHandle) -> SolidResult<Option<StaticHandle>> {
        match surface.0 {
            2002 => Ok(Some(StaticHandle(2000))),
            2003 => Ok(Some(StaticHandle(2001))),
            _ => Err(static_error()),
        }
    }
    fn face_surface(&self, face: StaticHandle) -> SolidResult<StaticHandle> {
        match face.0 {
            2000 => Ok(StaticHandle(2002)),
            2001 => Ok(StaticHandle(2003)),
            _ => Err(static_error()),
        }
    }
    fn surface(&self, surface: StaticHandle) -> SolidResult<&Surface> {
        match surface.0 {
            2002 => self.surfaces.first(),
            2003 => self.surfaces.get(1),
            _ => None,
        }
        .ok_or_else(static_error)
    }
    fn face_uv_domain(&self, face: StaticHandle) -> SolidResult<Extent2d> {
        self.face_surface(face)?;
        Ok(Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0))
    }
    fn face_tolerance(&self, face: StaticHandle) -> SolidResult<f64> {
        self.face_surface(face)?;
        Ok(1e-5)
    }
    fn surface_owner(&self, surface: StaticHandle) -> SolidResult<Option<StaticHandle>> {
        self.surface_face(surface)
    }
    fn shape_member(&self, _: StaticHandle) -> ShapeMember {
        ShapeMember::Other
    }
}
fn static_rust(case: &Case) -> Result<Output, HarnessError> {
    let input = static_input(case)?;
    // Validate the Rust fixture independently against the native protocol guard.
    // Do not invoke C++ here: the two sides must construct their own objects.
    let i: StaticInputs = case.parse()?;
    if input.len() > 396
        || i.first.is_empty()
        || i.second.is_empty()
        || i.first.len() > 64
        || i.second.len() > 64
    {
        return Err(HarnessError::new("unbounded static fixture"));
    }
    let mut kernel = Kernel::default();
    let first: Vec<Point3> = i
        .first
        .iter()
        .map(|p| Point3::new(p[0], p[1], p[2]))
        .collect();
    let second: Vec<Point3> = i
        .second
        .iter()
        .map(|p| Point3::new(p[0], p[1], p[2]))
        .collect();
    let mut all = first.clone();
    all.extend(&second);
    let tree1 = static_tree(&first, 0, 1e-5).map_err(|e| HarnessError::new(e.to_string()))?;
    let tree2 =
        static_tree(&second, first.len(), 1e-5).map_err(|e| HarnessError::new(e.to_string()))?;
    let empty_first = Tree::new_spatial(
        &tree1
            .node(
                tree1
                    .top_node()
                    .ok_or_else(|| HarnessError::new("missing fixture root"))?,
            )
            .map_err(|e| HarnessError::new(e.to_string()))?
            .bbox,
    );
    let empty_second = Tree::new_spatial(
        &tree2
            .node(
                tree2
                    .top_node()
                    .ok_or_else(|| HarnessError::new("missing fixture root"))?,
            )
            .map_err(|e| HarnessError::new(e.to_string()))?
            .bbox,
    );
    let nurb = NlCurve::new(
        vec![
            CPoint::new(-1.0, 0.0, 0.0, NOW),
            CPoint::new(1.0, 0.0, 0.0, NOW),
        ],
        1,
        vec![0.0, 0.0, 1.0, 1.0],
    )
    .map_err(|e| HarnessError::new(e.to_string()))?;
    let scene = StaticScene {
        points: all,
        first: tree1,
        second: tree2,
        empty_first,
        empty_second,
        curve: Curve::from(BSplineCurve::from_nurb(nurb, 3, false)),
        edge_owner: i.entry == 7,
        surfaces: [0.0, 1.0].map(|z| {
            Surface::from(solidcore::surface::plane::Plane::new(
                Point3::new(0.0, 0.0, z),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                solidcore::geom::vector2d::Vec2::new(1.0, 1.0),
                &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
            ))
        }),
    };
    let vectors = [Vec3::new(0.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 0.0)];
    let options = TopologySolveOptions {
        operation: SolverOperation::from_code(i.operation)
            .ok_or_else(|| HarnessError::new("operation"))?,
        requested: SolutionRequested::from_code(i.request)
            .ok_or_else(|| HarnessError::new("request"))?,
        tolerance_3d: i.tolerance,
        best_answer_so_far_sq: static_best(i.operation),
        opt_vectors: &vectors,
    };
    let point = Point3::new(i.point[0], i.point[1], i.point[2]);
    let curve = CurveRef {
        curve: &scene.curve,
        handle: StaticHandle(999),
    };
    let interval = Extent1d::from_min_max(0.0, 1.0);
    let shape = Shape::new(&mut kernel, &solidcore::geom::object::NoBreps, &[], &[], &[], &[])
        .map_err(|e| HarnessError::new(e.to_string()))?;
    let result = match i.entry {
        0 => TopologySolver::brep_point_solve(
            &kernel,
            &scene,
            Some(StaticHandle(1000)),
            point,
            options,
        ),
        1 => TopologySolver::brep_curve_solve(
            &kernel,
            &scene,
            Some(StaticHandle(1000)),
            curve,
            &interval,
            options,
        ),
        2 => TopologySolver::brep_brep_solve(
            &kernel,
            &scene,
            Some(StaticHandle(1000)),
            Some(StaticHandle(1001)),
            options,
        ),
        3 => TopologySolver::shape_point_solve(&kernel, &scene, &shape, point, options),
        4 => TopologySolver::shape_curve_solve(&kernel, &scene, &shape, curve, &interval, options),
        5..=8 => {
            use solidcore::intersect::topology_solver::p1::{LocalSolver, LocalSolverKind};
            let mut ts = TopologySolver::new(
                &kernel,
                &scene,
                options.operation,
                options.requested,
                options.tolerance_3d,
                options.best_answer_so_far_sq,
                options.opt_vectors,
            );
            let kind = match i.entry {
                5 => LocalSolverKind::VertexFace,
                6 => LocalSolverKind::CurveFace,
                7 => LocalSolverKind::EdgeFace,
                _ => LocalSolverKind::FaceFace,
            };
            let mut local =
                LocalSolver::new(kind, i.swap, if i.included { None } else { Some(&shape) });
            if i.entry == 6 {
                local.set_curve(Some(curve), interval);
            }
            let (first, second) = match i.entry {
                5 => (StaticHandle(0), StaticHandle(2002)),
                6 | 7 => (StaticHandle(999), StaticHandle(2002)),
                _ => (StaticHandle(2002), StaticHandle(2003)),
            };
            let (first, second) = if i.swap {
                (second, first)
            } else {
                (first, second)
            };
            local
                .solve_it(&kernel, &mut ts, Some(first), Some(second))
                .map(|()| ts.base.solutions)
        }
        _ => return Err(HarnessError::new("static entry")),
    };
    let mut flat = vec![0.0, 1e-5, 1e-5];
    flat.extend(scene.points.iter().map(|_| 1e-5));
    match result {
        Ok(solutions) => {
            flat.push(solutions.len() as f64);
            for s in solutions.as_slice() {
                flat.extend([
                    f64::from(s.solution_type.code()),
                    s.num_variables as f64,
                    s.num_objects as f64,
                    s.objects[0].map_or(-1.0, static_object_id),
                    s.objects[1].map_or(-1.0, static_object_id),
                    s.start.solution_value,
                ]);
                flat.extend(s.start.parameters.iter().take(s.num_variables).copied());
                if s.solution_type == SolutionType::RangeOfValues {
                    flat.push(s.end.solution_value);
                    flat.extend(s.end.parameters.iter().take(s.num_variables).copied());
                }
                flat.extend(
                    s.nodes
                        .iter()
                        .map(|node| f64::from(u8::from(node.is_some()))),
                );
            }
        }
        Err(e) => {
            if let Some(deferred) = Output::unported(&e) {
                return Ok(deferred);
            }
            let status = match e {
                SolidError::InvalidInput { .. } => 1007.0,
                SolidError::Upstream { code, .. } => f64::from(code),
                _ => return Err(HarnessError::new(e.to_string())),
            };
            if let Some(slot) = flat.first_mut() {
                *slot = status;
            }
            flat.push(0.0);
        }
    }
    Ok(static_output(&flat))
}
