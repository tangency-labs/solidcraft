//! The topology solver's local traversal and remaining solver operations.
//!
//! Upstream: `source/SMLib/src/SmTopologySolver.cpp`, `source/SMLib/inc/SmTopologySolver.h`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`TopologySolver::local_solve_branch`] | `SmTopologySolver::LocalSolve` |
//! | `LocalSolver::solve_vertex_face` | `SmTLSVertexFace::SolveIt` |
//! | `LocalSolver::solve_curve_vertex` | `SmTLSCurveVertex::SolveIt` |
//!
//! Deviations: topology pointers are typed object handles; the local solver is
//! temporarily taken from the parent and restored even when its solve fails.

use super::p1::TopologyObjects;
use super::p1::{self, best_answer_so_far, CurveRef};
use super::p1::{LocalSolver, LocalSolverKind, TopologySolver};
use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::Branch;
use crate::geom::global_solver::{GlobalSolve, SolutionRequested, SolverOperation};
use crate::geom::object::{Shape, ShapeMember};
use crate::geom::solution_array::SolutionObject;
use crate::geom::solution_array::{Solution, SolutionArray, SolutionType, SortKey};
use crate::geom::tol::are_same;
use crate::geom::tree::Tree;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec2};
use crate::nurbs::b_spline_curve::BoundingBoxRequest;
use crate::surface::surface::{p2::surface_cache, Surface};
use crate::surface::trim_srf_cache::POINT_CLASS_UNKNOWN;

const OP_LOCAL: &str = "intersect::topology_solver::p2::TopologySolver::local_solve_branch";
const OP_VF: &str = "intersect::topology_solver::p2::LocalSolver::solve_vertex_face";
const OP_CV: &str = "intersect::topology_solver::p2::LocalSolver::solve_curve_vertex";

impl<'t, O: SolutionObject + 't> TopologySolver<'t, '_, O> {
    /// Solves the object pairs of the two branch nodes in their native list order
    /// (`LocalSolve`), asking for further subdivision when either has child one.
    ///
    /// # Errors
    /// Invalid branches or object lists, missing local solver, and local-solve errors.
    pub fn local_solve_branch(&mut self, branch: &Branch) -> Result<bool> {
        let tree1 = self.base.tree(0)?;
        let tree2 = self.base.tree(1)?;
        let node1 = self.base.branch_node(branch, 0)?;
        let node2 = self.base.branch_node(branch, 1)?;
        let id1 = branch
            .first()
            .copied()
            .flatten()
            .ok_or(SolidError::null_pointer(OP_LOCAL))?;
        let id2 = branch
            .get(1)
            .copied()
            .flatten()
            .ok_or(SolidError::null_pointer(OP_LOCAL))?;
        for list1 in tree1.node_object_list(id1)? {
            let object1 = tree1.list(list1)?;
            if !self.may_contain_answer(&object1.bbox, &node2.bbox)? {
                continue;
            }
            for list2 in tree2.node_object_list(id2)? {
                let object2 = tree2.list(list2)?;
                if !self.may_contain_answer(&object1.bbox, &object2.bbox)? {
                    continue;
                }
                let mut local = self
                    .local_solver
                    .take()
                    .ok_or(SolidError::null_pointer(OP_LOCAL))?;
                let result = local.solve_it(self, object1.object, object2.object);
                self.local_solver = Some(local);
                result?;
            }
        }
        Ok(node1.child1.is_some() || node2.child1.is_some())
    }
}

impl<'a, O: SolutionObject> LocalSolver<'a, O> {
    pub(super) fn solve_vertex_face(
        &mut self,
        ts: &mut TopologySolver<'_, 'a, O>,
        object1: Option<O>,
        object2: Option<O>,
    ) -> Result<()> {
        let vertex = if self.swap_order { object2 } else { object1 }
            .ok_or(SolidError::null_pointer(OP_VF))?;
        self.point_vertex = Some(vertex);
        self.point = ts.objects.vertex_point(vertex)?;
        self.solve_point_face(ts, object1, object2)
    }

    pub(super) fn solve_curve_vertex(
        &mut self,
        ts: &mut TopologySolver<'_, 'a, O>,
        _object1: Option<O>,
        object2: Option<O>,
    ) -> Result<()> {
        let mut point_curve = Self::new(LocalSolverKind::PointCurve, !self.swap_order, self.shape);
        if let Some(edge) = self.curve_edge {
            point_curve.curve_owner = Some(edge);
            point_curve.interval = ts.objects.edge_interval(edge)?;
        } else {
            point_curve.interval = self.interval;
        }
        let vertex = object2.ok_or(SolidError::null_pointer(OP_CV))?;
        point_curve.point_vertex = Some(vertex);
        point_curve.point = ts.objects.vertex_point(vertex)?;
        let curve = self.curve.ok_or(SolidError::null_pointer(OP_CV))?;
        point_curve.point_curve(ts, curve.curve, Some(curve.handle))
    }
}

const OP_EV: &str = "intersect::topology_solver::p2::LocalSolver::solve_edge_vertex";
const OP_CF: &str = "intersect::topology_solver::p2::LocalSolver::solve_curve_face";
const OP_EF: &str = "intersect::topology_solver::p2::LocalSolver::solve_edge_face";
const OP_FF: &str = "intersect::topology_solver::p2::LocalSolver::solve_face_face";

fn swap_curve_surface_parameters<O>(solution: &mut Solution<O>) -> Result<()> {
    solution
        .start
        .parameters
        .get_mut(..3)
        .ok_or(SolidError::sm_err(OP_CF))?
        .rotate_left(1);
    if solution.solution_type == SolutionType::RangeOfValues {
        solution
            .end
            .parameters
            .get_mut(..3)
            .ok_or(SolidError::sm_err(OP_CF))?
            .rotate_left(1);
    }
    Ok(())
}

fn name_curve_face<O: Copy + PartialEq>(
    solution: &mut Solution<O>,
    objects: [Option<O>; 2],
    surface: O,
    face: O,
) {
    solution.objects[0] = if objects[0] == Some(surface) {
        Some(face)
    } else {
        objects[0]
    };
    solution.objects[1] = if objects[1] == Some(surface) {
        Some(face)
    } else {
        objects[1]
    };
    solution.num_objects = 2;
    solution.num_variables = 3;
}

fn surface_curve_intersect<O>(
    _surface: &Surface,
    _domain: &Extent2d,
    _curve: &Curve,
    _interval: &Extent1d,
    _tol: f64,
) -> Result<SolutionArray<O>> {
    // UNPORTED(surface.surface_p8): SmSurface::GlobalCurveIntersect
    Err(SolidError::unported(
        "SmSurface::GlobalCurveIntersect",
        "surface.surface_p8",
    ))
}

fn classified_curve_intervals<O>(
    _curve: &Curve,
    _interval: &Extent1d,
    _face: O,
    _tol: f64,
) -> Result<Vec<(Extent1d, i32)>> {
    // The overload taking interval storage is owned by curve_class_p4, not the
    // other constructor currently being ported in curve_class_p1.
    // UNPORTED(intersect.curve_class_p4): SmCurveClassification::SmCurveClassification
    Err(SolidError::unported(
        "SmCurveClassification::SmCurveClassification",
        "intersect.curve_class_p4",
    ))
}

fn untrimmed_range_endpoints<O: Copy + PartialEq>(
    kernel: &Kernel,
    surface: &Surface,
    surface_handle: O,
    _face_domain: &Extent2d,
    _start: Point3,
    _end: Point3,
    _tol: f64,
) -> Result<Option<(Vec2, Vec2)>> {
    // The cache must be checked out and point_test_enabled temporarily disabled
    // around BOTH GlobalPointSolve calls; restore it on success and error.
    // get_surface_cache now constructs an owned cache. This operation needs the
    // shared cache checkout consumed by BOTH point solves, so disabling a flag
    // on that independent owned copy would not disable trimming in the solver.
    // No endpoint is synthesized while that shared-cache bridge is unavailable.
    let _cache = surface_cache(kernel, surface, Some(surface_handle))?;
    // UNPORTED(surface.surface_p1): SmSurface::CacheMakeOrValidate (cache checkout bridge)
    Err(SolidError::unported(
        "SmSurface::CacheMakeOrValidate",
        "surface.surface_p1",
    ))
}

impl<'a, O: SolutionObject> LocalSolver<'a, O> {
    pub(super) fn solve_edge_vertex(
        &mut self,
        ts: &mut TopologySolver<'_, 'a, O>,
        object1: Option<O>,
        object2: Option<O>,
    ) -> Result<()> {
        let vertex = if self.swap_order { object1 } else { object2 }
            .ok_or(SolidError::null_pointer(OP_EV))?;
        // Upstream selects this SAME object as pCurve (SmTopologySolver.cpp:1032):
        // for a real vertex that cast is invalid. Validate the selected object class
        // instead of reinterpreting memory or silently correcting the operand.
        let curve_object = if self.swap_order { object1 } else { object2 }
            .ok_or(SolidError::null_pointer(OP_EV))?;
        if curve_object.kind() != crate::geom::solution_array::ObjectKind::Curve {
            return Err(SolidError::invalid_input(
                OP_EV,
                "upstream selects a vertex as a curve",
            ));
        }
        let _ = ts.objects.curve(curve_object)?;
        let edge = ts.objects.curve_edge(curve_object)?;
        if self.in_shape(ts.objects, edge) {
            let edge = edge.ok_or(SolidError::invalid_input(
                OP_EV,
                "upstream selects a vertex as a curve",
            ))?;
            self.curve_edge = Some(edge);
            self.interval = ts.objects.edge_interval(edge)?;
            self.curve = match ts.objects.edge_curve(edge)? {
                Some(handle) => Some(CurveRef {
                    curve: ts.objects.curve(handle)?,
                    handle,
                }),
                None => None,
            };
            self.solve_curve_vertex(ts, Some(vertex), Some(vertex))?;
        }
        Ok(())
    }

    pub(super) fn solve_edge_face(
        &mut self,
        ts: &mut TopologySolver<'_, 'a, O>,
        object1: Option<O>,
        object2: Option<O>,
    ) -> Result<()> {
        let curve = if self.swap_order { object2 } else { object1 }
            .ok_or(SolidError::null_pointer(OP_EF))?;
        let edge = ts.objects.curve_edge(curve)?;
        if self.in_shape(ts.objects, edge) {
            let edge = edge.ok_or(SolidError::invalid_input(OP_EF, "curve without an edge"))?;
            self.curve_edge = Some(edge);
            self.interval = ts.objects.edge_interval(edge)?;
            self.curve = match ts.objects.edge_curve(edge)? {
                Some(handle) => Some(CurveRef {
                    curve: ts.objects.curve(handle)?,
                    handle,
                }),
                None => None,
            };
            self.solve_curve_face(ts, object1, object2)?;
        }
        Ok(())
    }

    pub(super) fn solve_curve_face(
        &self,
        ts: &mut TopologySolver<'_, '_, O>,
        object1: Option<O>,
        object2: Option<O>,
    ) -> Result<()> {
        let surface_handle = if self.swap_order { object1 } else { object2 }
            .ok_or(SolidError::null_pointer(OP_CF))?;
        let owner = self
            .curve_edge
            .or(self.curve_face)
            .or(self.curve.map(|c| c.handle));
        let named = if self.swap_order {
            [Some(surface_handle), owner]
        } else {
            [owner, Some(surface_handle)]
        };
        let face = ts.objects.surface_face(surface_handle)?;
        if !self.in_shape(ts.objects, face) {
            return Ok(());
        }
        let face = face.ok_or(SolidError::invalid_input(OP_CF, "surface without a face"))?;
        let curve = self.curve.ok_or(SolidError::null_pointer(OP_CF))?;
        if ts.base.projected_operation {
            let (curves, uv_curves) = p1::face_silhouettes(
                ts.kernel,
                ts.objects,
                face,
                &ts.base.opt_vectors,
                ts.base.tolerance_3d,
            )?;
            for (i, other) in curves.iter().enumerate() {
                let mut local =
                    LocalSolver::new(LocalSolverKind::CurveCurve, self.swap_order, self.shape);
                local.curve = Some(curve);
                local.curve_edge = self.curve_edge;
                local.curve_face = self.curve_face;
                local.interval = self.interval;
                local.surface_curve = self.surface_curve;
                // The face owner replaces this temporary curve in every result;
                // its placeholder handle is never used as a curve lookup.
                local.curve2 = Some(CurveRef {
                    curve: other,
                    handle: face,
                });
                local.curve_edge2 = None;
                local.interval2 = other.natural_interval();
                local.curve_face2 = Some(face);
                local.surface_curve2 = Some(uv_curves.get(i).ok_or(SolidError::sm_err(OP_CF))?);
                local.solve_curve_curve(ts)?;
            }
            return Ok(());
        }
        let best = best_answer_so_far(&ts.base);
        let face_surface_handle = ts.objects.face_surface(face)?;
        let surface = ts.objects.surface(face_surface_handle)?;
        let domain = ts.objects.face_uv_domain(face)?;
        let tol = ts.base.tolerance_3d + ts.objects.face_tolerance(face)?;
        let solutions = if ts.base.operation == SolverOperation::Intersect {
            surface_curve_intersect(surface, &domain, curve.curve, &self.interval, tol)?
        } else {
            let mut vectors = ts.base.opt_vectors.clone();
            if ts.base.operation == SolverOperation::Signed3dDirectedMinimize && self.swap_order {
                let vector = vectors
                    .first_mut()
                    .ok_or(SolidError::invalid_input(OP_CF, "missing direction vector"))?;
                *vector = -*vector;
                vectors.truncate(1);
            }
            surface.global_curve_solve(
                &domain,
                curve.curve,
                &self.interval,
                ts.base.operation,
                tol,
                best,
                &vectors,
                SolutionRequested::All,
            )?
        };
        for found in solutions.as_slice() {
            let mut solution = *found;
            if ts.base.operation == SolverOperation::Intersect {
                if solution.solution_type == SolutionType::RangeOfValues
                    && !are_same(solution.start.param(0)?, solution.end.param(0)?)
                {
                    let interval =
                        Extent1d::from_min_max(solution.start.param(0)?, solution.end.param(0)?);
                    let intervals = classified_curve_intervals(
                        curve.curve,
                        &interval,
                        face,
                        ts.base.tolerance_3d,
                    )?;
                    for (interval, class) in intervals {
                        // Keep only SM_PC_FACE / SM_PC_EDGE intervals.
                        if class != crate::surface::trim_srf_cache::POINT_CLASS_FACE
                            && class != crate::surface::trim_srf_cache::POINT_CLASS_EDGE
                        {
                            continue;
                        }
                        solution.start.parameters[0] = interval.min();
                        solution.end.parameters[0] = interval.max();
                        let start = curve.curve.evaluate_point(interval.min())?;
                        let end = curve.curve.evaluate_point(interval.max())?;
                        if let Some((uv_start, uv_end)) = untrimmed_range_endpoints(
                            ts.kernel,
                            surface,
                            surface_handle,
                            &domain,
                            start,
                            end,
                            2.0 * tol,
                        )? {
                            solution.start.parameters[1] = uv_start.x;
                            solution.start.parameters[2] = uv_start.y;
                            solution.end.parameters[1] = uv_end.x;
                            solution.end.parameters[2] = uv_end.y;
                        } else {
                            continue;
                        }
                        name_curve_face(&mut solution, named, surface_handle, face);
                        if self.swap_order {
                            swap_curve_surface_parameters(&mut solution)?;
                        }
                        ts.base
                            .add_sorted_solution(&solution, SortKey::BySolutionValue, false)?;
                    }
                    continue;
                }
                let src_tol = p1::object_zone_tol_3d(
                    self.curve_edge
                        .or(self.curve_face)
                        .or(self.curve.map(|c| c.handle)),
                )?;
                let uv = Vec2::new(solution.start.param(1)?, solution.start.param(2)?);
                if p1::face_point_classify(face, uv, src_tol, false, false)? == POINT_CLASS_UNKNOWN
                {
                    continue;
                }
            }
            name_curve_face(&mut solution, named, surface_handle, face);
            if self.swap_order {
                swap_curve_surface_parameters(&mut solution)?;
            }
            ts.base
                .add_sorted_solution(&solution, SortKey::BySolutionValue, true)?;
        }
        Ok(())
    }

    pub(super) fn solve_face_face(
        &self,
        ts: &mut TopologySolver<'_, '_, O>,
        object1: Option<O>,
        object2: Option<O>,
    ) -> Result<()> {
        let surface1 = object1.ok_or(SolidError::null_pointer(OP_FF))?;
        let surface2 = object2.ok_or(SolidError::null_pointer(OP_FF))?;
        let face1 = ts.objects.surface_face(surface1)?;
        let face2 = ts.objects.surface_face(surface2)?;
        if !self.in_shape(ts.objects, face1) || !self.in_shape(ts.objects, face2) {
            return Ok(());
        }
        let face1 = face1.ok_or(SolidError::invalid_input(
            OP_FF,
            "first surface without a face",
        ))?;
        let face2 = face2.ok_or(SolidError::invalid_input(
            OP_FF,
            "second surface without a face",
        ))?;
        let best = best_answer_so_far(&ts.base);
        if ts.base.projected_operation {
            let (curves, uv_curves) = p1::face_silhouettes(
                ts.kernel,
                ts.objects,
                face1,
                &ts.base.opt_vectors,
                ts.base.tolerance_3d,
            )?;
            for (i, curve) in curves.iter().enumerate() {
                let mut local =
                    LocalSolver::new(LocalSolverKind::CurveFace, self.swap_order, self.shape);
                local.curve = Some(CurveRef {
                    curve,
                    handle: face1,
                });
                local.curve_face = Some(face1);
                local.curve_edge = None;
                local.interval = curve.natural_interval();
                local.surface_curve = Some(uv_curves.get(i).ok_or(SolidError::sm_err(OP_FF))?);
                local.solve_curve_face(ts, Some(surface2), Some(surface2))?;
            }
        } else {
            let a = ts.objects.surface(surface1)?;
            let b = ts.objects.surface(surface2)?;
            let solutions = a.global_surface_solve(
                &ts.objects.face_uv_domain(face1)?,
                b,
                &ts.objects.face_uv_domain(face2)?,
                ts.base.operation,
                ts.base.tolerance_3d,
                best,
                &ts.base.opt_vectors,
                ts.base.solution_requested,
            )?;
            for found in solutions.as_slice() {
                let mut solution = *found;
                solution.objects[0] = Some(face1);
                solution.objects[1] = Some(face2);
                solution.num_objects = 2;
                solution.num_variables = 4;
                ts.base
                    .add_sorted_solution(&solution, SortKey::BySolutionValue, true)?;
            }
        }
        Ok(())
    }
}

/// The three B-rep cache trees, with object handles resolved by the adapter.
/// These are the `GetVertexTree`, `GetCurveTree`, and `GetSurfaceTree` views.
#[derive(Debug, Clone, Copy)]
pub struct TopologyTrees<'t, O> {
    /// Vertices.
    pub vertices: &'t Tree<O>,
    /// Edge curves.
    pub curves: &'t Tree<O>,
    /// Face surfaces.
    pub surfaces: &'t Tree<O>,
}

/// Options shared by the static topology-solver entry points.
#[derive(Debug, Clone, Copy)]
pub struct TopologySolveOptions<'v> {
    /// Operation to solve.
    pub operation: SolverOperation,
    /// Which solutions to keep.
    pub requested: SolutionRequested,
    /// Geometric tolerance.
    pub tolerance_3d: f64,
    /// Initial squared best answer (including the signed convention).
    pub best_answer_so_far_sq: f64,
    /// Optional operation vectors, empty for the upstream null pointer.
    pub opt_vectors: &'v [crate::math::Vec3],
}

struct ShapeTreeViews<O> {
    vertices: Tree<O>,
    curves: Tree<O>,
    surfaces: Tree<O>,
}

impl<O: SolutionObject> ShapeTreeViews<O> {
    fn new(objects: &dyn TopologyObjects<O>, shape: &Shape) -> Result<Self> {
        Ok(Self {
            vertices: shape
                .vertex_tree()
                .map_objects(|key| objects.shape_member_object(ShapeMember::Vertex(key)))?,
            curves: shape
                .curve_tree()
                .map_objects(|key| objects.shape_member_object(ShapeMember::Edge(key)))?,
            surfaces: shape
                .surface_tree()
                .map_objects(|key| objects.shape_member_object(ShapeMember::Face(key)))?,
        })
    }
    fn views(&self) -> TopologyTrees<'_, O> {
        TopologyTrees {
            vertices: &self.vertices,
            curves: &self.curves,
            surfaces: &self.surfaces,
        }
    }
}

fn valid_point_operation(op: SolverOperation) -> bool {
    matches!(
        op,
        SolverOperation::Minimize
            | SolverOperation::Maximize
            | SolverOperation::Rayfire
            | SolverOperation::ProjectedMinimize
            | SolverOperation::ProjectedMaximize
            | SolverOperation::SignedDirectedMinimize
            | SolverOperation::Signed3dDirectedMinimize
            | SolverOperation::DirectedMinimize
            | SolverOperation::DirectedMaximize
            | SolverOperation::Intersect
            | SolverOperation::Normalize
    )
}

fn valid_curve_operation(op: SolverOperation, wireframe: bool) -> bool {
    (valid_point_operation(op) && op != SolverOperation::Rayfire)
        || (wireframe && op == SolverOperation::IntersectWireframe)
}

fn point_tree<O: Copy + PartialEq>(point: Point3) -> Tree<O> {
    Tree::from_point(point, 0.0, true)
}

fn curve_tree<O: SolutionObject>(
    k: &Kernel,
    curve: CurveRef<'_, O>,
    interval: &Extent1d,
) -> Result<Tree<O>> {
    const OP: &str = "intersect::topology_solver::p2::curve_tree";
    let mut tree = point_tree(Point3::new(0.0, 0.0, 0.0));
    let bbox = curve
        .curve
        .calculate_bounding_box(
            k,
            interval,
            BoundingBoxRequest {
                normal: true,
                ..BoundingBoxRequest::default()
            },
            true,
        )?
        .normal
        .ok_or(SolidError::sm_err(OP))?;
    let root = tree.top_node().ok_or(SolidError::sm_err(OP))?;
    tree.node_mut(root)?.bbox = bbox;
    let list = tree.node(root)?.list_head().ok_or(SolidError::sm_err(OP))?;
    let item = tree.list_mut(list)?;
    item.bbox = bbox;
    item.object = Some(curve.handle);
    Ok(tree)
}

fn new_topology_solver<'t, 'a, O: SolutionObject + 't>(
    kernel: &'a Kernel,
    objects: &'a dyn TopologyObjects<O>,
    options: TopologySolveOptions<'_>,
) -> TopologySolver<'t, 'a, O> {
    TopologySolver::new(
        kernel,
        objects,
        options.operation,
        options.requested,
        options.tolerance_3d,
        options.best_answer_so_far_sq,
        options.opt_vectors,
    )
}

fn early_point_answer<O: SolutionObject>(
    ts: &TopologySolver<'_, '_, O>,
    options: TopologySolveOptions<'_>,
) -> bool {
    options.requested == SolutionRequested::Single
        && ts
            .base
            .solutions
            .as_slice()
            .first()
            .is_some_and(|s| s.start.solution_value <= options.tolerance_3d)
}

fn rewrite_endpoint_solutions<O: SolutionObject>(
    solutions: &mut SolutionArray<O>,
    curve: O,
    parameter: f64,
) {
    for i in 0..solutions.len() {
        if let Some(solution) = solutions.get_mut(i) {
            if solution.num_objects == 1 && solution.num_variables == 2 {
                solution.num_objects = 2;
                solution.objects[1] = solution.objects[0];
                solution.objects[0] = Some(curve);
                solution.start.parameters[2] = solution.start.parameters[1];
                solution.start.parameters[1] = solution.start.parameters[0];
                solution.start.parameters[0] = parameter;
                solution.num_variables = 3;
            }
        }
    }
}

impl<'t, O: SolutionObject + 't> TopologySolver<'t, '_, O> {
    /// Solves a query point against a B-rep's vertex, edge and face trees
    /// (`BrepPointSolve`), retaining upstream's early single-answer exits.
    ///
    /// # Errors
    /// Invalid operation or null B-rep, unavailable B-rep cache, or a local solve.
    pub fn brep_point_solve(
        kernel: &Kernel,
        objects: &dyn TopologyObjects<O>,
        brep: Option<O>,
        point: Point3,
        options: TopologySolveOptions<'_>,
    ) -> Result<SolutionArray<O>> {
        const OP: &str = "intersect::topology_solver::p2::TopologySolver::brep_point_solve";
        if !valid_point_operation(options.operation) {
            return Err(SolidError::invalid_input(OP, "unsupported operation"));
        }
        let brep = brep.ok_or(SolidError::null_pointer(OP))?;
        let mut ts = new_topology_solver(kernel, objects, options);
        let trees = objects.brep_trees(brep)?;
        let query = point_tree(point);
        if options.operation != SolverOperation::Normalize {
            let mut vertices = LocalSolver::new(LocalSolverKind::PointVertex, false, None);
            vertices.set_point(point);
            ts.solve_trees(&query, trees.vertices, &mut vertices)?;
            if early_point_answer(&ts, options) {
                return Ok(ts.base.solutions);
            }
            let mut edges = LocalSolver::new(LocalSolverKind::PointEdge, false, None);
            edges.set_point(point);
            ts.solve_trees(&query, trees.curves, &mut edges)?;
            if early_point_answer(&ts, options) {
                return Ok(ts.base.solutions);
            }
        }
        let mut faces = LocalSolver::new(LocalSolverKind::PointFace, false, None);
        faces.set_point(point);
        ts.solve_trees(&query, trees.surfaces, &mut faces)?;
        Ok(ts.base.solutions)
    }

    /// Solves a point against the members of `shape` (`ShapePointSolve`).
    ///
    /// # Errors
    /// Invalid operation, handle mapping failure or a local solve.
    pub fn shape_point_solve(
        kernel: &Kernel,
        objects: &dyn TopologyObjects<O>,
        shape: &Shape,
        point: Point3,
        options: TopologySolveOptions<'_>,
    ) -> Result<SolutionArray<O>> {
        const OP: &str = "intersect::topology_solver::p2::TopologySolver::shape_point_solve";
        if !valid_point_operation(options.operation) {
            return Err(SolidError::invalid_input(OP, "unsupported operation"));
        }
        let mut ts = new_topology_solver(kernel, objects, options);
        let mapped = ShapeTreeViews::new(objects, shape)?;
        let trees = mapped.views();
        let query = point_tree(point);
        if options.operation != SolverOperation::Normalize {
            let mut vertices = LocalSolver::new(LocalSolverKind::PointVertex, false, Some(shape));
            vertices.set_point(point);
            ts.solve_trees(&query, trees.vertices, &mut vertices)?;
            let mut edges = LocalSolver::new(LocalSolverKind::PointEdge, false, Some(shape));
            edges.set_point(point);
            ts.solve_trees(&query, trees.curves, &mut edges)?;
        }
        let mut faces = LocalSolver::new(LocalSolverKind::PointFace, false, Some(shape));
        faces.set_point(point);
        ts.solve_trees(&query, trees.surfaces, &mut faces)?;
        Ok(ts.base.solutions)
    }

    /// Solves a curve against a B-rep (`BrepCurveSolve`). Wireframe intersection
    /// skips faces; signed directed distance additionally tests the start point.
    ///
    /// # Errors
    /// Invalid operation or null B-rep, cache, bounding-box or local-solve errors.
    pub fn brep_curve_solve(
        kernel: &Kernel,
        objects: &dyn TopologyObjects<O>,
        brep: Option<O>,
        curve: CurveRef<'_, O>,
        interval: &Extent1d,
        mut options: TopologySolveOptions<'_>,
    ) -> Result<SolutionArray<O>> {
        const OP: &str = "intersect::topology_solver::p2::TopologySolver::brep_curve_solve";
        if !valid_curve_operation(options.operation, true) {
            return Err(SolidError::invalid_input(OP, "unsupported operation"));
        }
        let wires_only = options.operation == SolverOperation::IntersectWireframe;
        if wires_only {
            options.operation = SolverOperation::Intersect;
        }
        let mut ts = new_topology_solver(kernel, objects, options);
        let trees = objects.brep_trees(brep.ok_or(SolidError::null_pointer(OP))?)?;
        let query = curve_tree(kernel, curve, interval)?;
        if options.operation != SolverOperation::Normalize {
            let mut vertices = LocalSolver::new(LocalSolverKind::CurveVertex, false, None);
            vertices.set_curve(Some(curve), *interval);
            ts.solve_trees(&query, trees.vertices, &mut vertices)?;
            let mut edges = LocalSolver::new(LocalSolverKind::CurveEdge, false, None);
            edges.set_curve(Some(curve), *interval);
            ts.solve_trees(&query, trees.curves, &mut edges)?;
        }
        if !wires_only {
            let mut faces = LocalSolver::new(LocalSolverKind::CurveFace, false, None);
            faces.set_curve(Some(curve), *interval);
            ts.solve_trees(&query, trees.surfaces, &mut faces)?;
            if options.operation == SolverOperation::Signed3dDirectedMinimize {
                let parameter = interval.min(); // upstream currently tests only the start
                let point = curve.curve.evaluate_point(parameter)?;
                let query = point_tree(point);
                let mut faces = LocalSolver::new(LocalSolverKind::PointFace, false, None);
                faces.set_point(point);
                let mut scoped = TopologySolver {
                    base: ts.take_base(),
                    kernel,
                    objects,
                    local_solver: None,
                };
                let result = scoped.solve_trees(&query, trees.surfaces, &mut faces);
                ts.restore_base(scoped.base, trees.surfaces);
                result?;
                rewrite_endpoint_solutions(&mut ts.base.solutions, curve.handle, parameter);
            }
        }
        Ok(ts.base.solutions)
    }

    /// Solves a curve against `shape` (`ShapeCurveSolve`).
    ///
    /// # Errors
    /// Invalid operation, handle mapping, curve bounding box or local solve.
    pub fn shape_curve_solve(
        kernel: &Kernel,
        objects: &dyn TopologyObjects<O>,
        shape: &Shape,
        curve: CurveRef<'_, O>,
        interval: &Extent1d,
        options: TopologySolveOptions<'_>,
    ) -> Result<SolutionArray<O>> {
        const OP: &str = "intersect::topology_solver::p2::TopologySolver::shape_curve_solve";
        if !valid_curve_operation(options.operation, false) {
            return Err(SolidError::invalid_input(OP, "unsupported operation"));
        }
        let mut ts = new_topology_solver(kernel, objects, options);
        let mapped = ShapeTreeViews::new(objects, shape)?;
        let trees = mapped.views();
        let query = curve_tree(kernel, curve, interval)?;
        if options.operation != SolverOperation::Normalize {
            let mut vertices = LocalSolver::new(LocalSolverKind::CurveVertex, false, Some(shape));
            vertices.set_curve(Some(curve), *interval);
            ts.solve_trees(&query, trees.vertices, &mut vertices)?;
            let mut edges = LocalSolver::new(LocalSolverKind::CurveEdge, false, Some(shape));
            edges.set_curve(Some(curve), *interval);
            ts.solve_trees(&query, trees.curves, &mut edges)?;
        }
        let mut faces = LocalSolver::new(LocalSolverKind::CurveFace, false, Some(shape));
        faces.set_curve(Some(curve), *interval);
        ts.solve_trees(&query, trees.surfaces, &mut faces)?;
        if options.operation == SolverOperation::Signed3dDirectedMinimize {
            let parameter = interval.min();
            let point = curve.curve.evaluate_point(parameter)?;
            let query = point_tree(point);
            let mut faces = LocalSolver::new(LocalSolverKind::PointFace, false, Some(shape));
            faces.set_point(point);
            let mut scoped = TopologySolver {
                base: ts.take_base(),
                kernel,
                objects,
                local_solver: None,
            };
            let result = scoped.solve_trees(&query, trees.surfaces, &mut faces);
            ts.restore_base(scoped.base, trees.surfaces);
            result?;
            rewrite_endpoint_solutions(&mut ts.base.solutions, curve.handle, parameter);
        }
        Ok(ts.base.solutions)
    }

    /// Performs the ordered vertex/edge/face comparisons of `sm_SolveTwoTrees`.
    /// Normalize skips every comparison except face/face.
    ///
    /// # Errors
    /// Tree traversal or local-solver errors.
    pub fn solve_two_tree_sets(
        &mut self,
        first: TopologyTrees<'t, O>,
        second: TopologyTrees<'t, O>,
    ) -> Result<()> {
        if self.base.operation != SolverOperation::Normalize {
            for (kind, swap, tree1, tree2) in [
                (
                    LocalSolverKind::VertexVertex,
                    false,
                    first.vertices,
                    second.vertices,
                ),
                (
                    LocalSolverKind::VertexEdge,
                    false,
                    first.vertices,
                    second.curves,
                ),
                (
                    LocalSolverKind::VertexEdge,
                    true,
                    first.curves,
                    second.vertices,
                ),
                (
                    LocalSolverKind::EdgeEdge,
                    false,
                    first.curves,
                    second.curves,
                ),
                (
                    LocalSolverKind::VertexFace,
                    false,
                    first.vertices,
                    second.surfaces,
                ),
                (
                    LocalSolverKind::VertexFace,
                    true,
                    first.surfaces,
                    second.vertices,
                ),
                (
                    LocalSolverKind::EdgeFace,
                    false,
                    first.curves,
                    second.surfaces,
                ),
                (
                    LocalSolverKind::EdgeFace,
                    true,
                    first.surfaces,
                    second.curves,
                ),
            ] {
                let mut local = LocalSolver::new(kind, swap, None);
                self.solve_two_trees(tree1, tree2, &mut local)?;
            }
        }
        let mut faces = LocalSolver::new(LocalSolverKind::FaceFace, false, None);
        self.solve_two_trees(first.surfaces, second.surfaces, &mut faces)
    }

    /// Solves two B-reps using their cache trees (`BrepBrepSolve`).
    ///
    /// # Errors
    /// Invalid operation or null B-reps, unavailable caches or local-solve errors.
    pub fn brep_brep_solve(
        kernel: &Kernel,
        objects: &dyn TopologyObjects<O>,
        first: Option<O>,
        second: Option<O>,
        options: TopologySolveOptions<'_>,
    ) -> Result<SolutionArray<O>> {
        const OP: &str = "intersect::topology_solver::p2::TopologySolver::brep_brep_solve";
        if !valid_curve_operation(options.operation, false) {
            return Err(SolidError::invalid_input(OP, "unsupported operation"));
        }
        let mut ts = new_topology_solver(kernel, objects, options);
        let first = objects.brep_trees(first.ok_or(SolidError::null_pointer(OP))?)?;
        let second = objects.brep_trees(second.ok_or(SolidError::null_pointer(OP))?)?;
        ts.solve_two_tree_sets(first, second)?;
        Ok(ts.base.solutions)
    }
}
