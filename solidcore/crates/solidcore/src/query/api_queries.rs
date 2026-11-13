//! Public closest-point, boundary, bounding-box and classification queries.
//!
//! Upstream: `source/SM_API/src/SmApiQueries.cpp`, `source/SM_API/inc/SmApiQueries.h`,
//! `source/SMLib/src/SmSolutionArray.cpp` (solution point extraction).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`curve_closest_point`], [`curve_closest_point_all`] | `SmApiCurveClosestPoint`, `SmApiCurveClosestPointAll` |
//! | [`surface_closest_point`] | `SmApiSurfaceClosestPoint` |
//! | [`get_closest_point`], [`brep_closest_point`], [`brep_closest_point_all`] | `SmApiGetClosestPoint`, `SmApiBrepClosestPoint`, `SmApiBrepClosestPointAll` |
//! | [`brep_relationship`], [`brep_distance`] | `SmApiBrepRelationship`, `SmApiBrepDistance` |
//! | [`brep_relationship_solve`], [`brep_relationship_classify`] | the solve and the classification of `SmApiBrepRelationship` |
//! | [`get_edges`], [`get_faces`], [`find_edge`], [`find_faces`] | `SmApiGetEdges`, `SmApiGetFaces`, `SmApiFindEdge`, `SmApiFindFaces` |
//! | [`face_get_boundary_loops`] | `SmApiFaceGetBoundaryLoops` |
//! | [`brep_is_manifold_solid`], [`brep_bounding_box`] | `SmApiBrepIsManifoldSolid`, `SmApiBrepBoundingBox` |
//! | [`brep_material_census`] | `SmApiBrepMaterialCensus` |
//! | [`face_bounding_box`], [`face_internal_point`], [`face_classify_uv`], [`face_closest_point`], [`face_closest_point_in_shape`] | `SmApiFaceBoundingBox`, `SmApiFaceInternalPoint`, `SmApiFaceClassifyUV`, `SmApiFaceClosestPoint` |
//! | [`edge_bounding_box`], [`edge_closest_point`], [`edge_parameter_range`], [`edge_classify_parameter`], [`edge_tangent`] | `SmApiEdgeBoundingBox`, `SmApiEdgeClosestPoint`, `SmApiEdgeParameterRange`, `SmApiEdgeClassifyParameter`, `SmApiEdgeTangent` |
//! | [`vertex_get_point`] | `SmApiVertexGetPoint` |
//! | [`point_distance`], [`vertex_distance`], [`edge_face_gap_samples`], [`edgeuse_face_frame`] | not upstream: certified distances (the last on `SmCrvSrfGapFunction`'s samples) |
//! | `count_contained_samples`, `edge_piece_points`, `point_in_solid`, `target_curve_crossings` (private) | `sm_CountContainedSamples`, `sm_PointInSolid`, its `BrepCurveSolve` |
//! | `solution_point` (private) | `SmSolution::GetPoint` |
//!
//! Deviations: typed references/ids replace nullable pointers; results replace
//! output parameters (including append-only arrays), so errors expose no partial
//! arrays. Callers supply the Kernel and topology-solver object adapter/cache (an
//! adapter holding the B-reps mutably calls the split relationship and face
//! closest-point steps, so the solve can make face caches on them);
//! handles retain the caller's identity. Missing topology queries return Unported
//! at the call site. Solution point extraction retains upstream's zero parameter
//! advance for a preceding Surface and ignores ordinary evaluation errors (the
//! output stays undefined); Unported errors still propagate. The containment tests of
//! [`brep_relationship`] (`sm_PointInSolid`, `sm_CountContainedSamples`) classify on a
//! private copy of each target ([`BrepClassifier::of_brep`], keyed by the target's
//! recorded naming scheme and proven owner keys); `sm_CountContainedSamples`'
//! `SmApiFaceInternalPoint` of each sample face runs on a copy of the sample B-rep
//! (borrowed next to the target), and its `BrepCurveSolve` of each sample edge runs
//! through the caller's objects on the edge curves of the sample's curve tree (so in
//! that tree's order, which only the counts read), while the public
//! [`face_internal_point`] works on its B-rep in place. No new recursion or allocation from
//! serialized counts.

use crate::curve::curve::p1::GlobalPointSolveOptions;
use crate::curve::curve::Curve;
use crate::error::{ignored, EntityRef, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent3d::Extent3d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::object::{BrepStore, FaceKey, ObjectRef, Shape, ShapeMember};
use crate::geom::solution_array::{
    ObjectKind, Solution, SolutionArray, SolutionObject, SolutionType,
};
use crate::geom::tol::{context_xsect_tol_3d, SmallTopoType};
use crate::geom::tools::{self, EdgePick};
use crate::heal::gap::GapFunction;
use crate::intersect::point_class::{PointClassType, PointClassification};
use crate::intersect::topology_solver::p1::{CurveRef, TopologyObjects, TopologySolver};
use crate::intersect::topology_solver::p2::TopologySolveOptions;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3, EFF_ZERO};
use crate::nurbs::b_spline_curve::BoundingBoxRequest;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::brep::BrepClassifier;
use crate::query::brep::{self as qbrep, BoundingBoxOptions};
use crate::query::normal::{certify as certify_normal, Sense, UnitVector};
use crate::surface::surface::p1::zone_tol_3d;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::Surface;
use crate::topology::brep_cache::CacheObject;
use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use crate::topology::{
    edge, edgeuse::p1 as eu, face::p2 as face, Brep, BrepId, EdgeId, EdgeuseId, FaceId, FaceuseId,
    VertexId,
};

const OP: &str = "query::api_queries";

/// A closest spatial point and its upstream (unsquared) solution distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClosestPoint {
    /// Point on the queried entity.
    pub point: Point3,
    /// Distance supplied by the solver.
    pub distance: f64,
}

/// A closest curve point with its parameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveClosestPoint {
    /// Spatial result.
    pub closest: ClosestPoint,
    /// Curve parameter.
    pub parameter: f64,
}

/// One spatial point and all returned UV representations of that point.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceClosestPoint {
    /// Spatial result.
    pub closest: ClosestPoint,
    /// UVs in solver order, including seam representations.
    pub uvs: Vec<Point2>,
}

#[derive(Clone, Copy, PartialEq)]
struct CurveHandle(u8, Option<Extent1d>);
impl SolutionObject for CurveHandle {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }
    fn natural_interval(&self) -> Option<Extent1d> {
        self.1
    }
}

fn curve_solutions(
    k: &Kernel,
    curve: &Curve,
    point: Point3,
    requested: SolutionRequested,
) -> Result<SolutionArray<CurveHandle>> {
    curve.global_point_solve(
        k,
        &curve.natural_interval(),
        SolverOperation::Minimize,
        point,
        crate::geom::tol::ZONE_TOL_3D,
        requested,
        &GlobalPointSolveOptions {
            handle: Some(CurveHandle(0, Some(curve.natural_interval()))),
            analytic_handle: Some(CurveHandle(1, None)),
            ..GlobalPointSolveOptions::default()
        },
    )
}

fn curve_result<O: Copy>(
    k: &Kernel,
    curve: &Curve,
    solution: &Solution<O>,
) -> Result<CurveClosestPoint> {
    let parameter = solution.start.param(0)?;
    let point = curve.evaluate_point(k, parameter)?;
    Ok(CurveClosestPoint {
        closest: ClosestPoint {
            point,
            distance: solution.start.solution_value,
        },
        parameter,
    })
}

/// Closest point on the curve's natural interval (`SmApiCurveClosestPoint`).
/// Solver and evaluation errors propagate; an empty solution set returns `SM_ERR`.
pub fn curve_closest_point(k: &Kernel, curve: &Curve, point: Point3) -> Result<CurveClosestPoint> {
    let solutions = curve_solutions(k, curve, point, SolutionRequested::Single)?;
    curve_result(
        k,
        curve,
        solutions
            .as_slice()
            .first()
            .ok_or_else(|| SolidError::query_failed(OP, "the curve has no closest point"))?,
    )
}

/// All closest curve solutions, retaining solver order (`SmApiCurveClosestPointAll`).
/// An empty solution set or an evaluation failure returns an error.
pub fn curve_closest_point_all(
    k: &Kernel,
    curve: &Curve,
    point: Point3,
) -> Result<Vec<CurveClosestPoint>> {
    let solutions = curve_solutions(k, curve, point, SolutionRequested::All)?;
    if solutions.is_empty() {
        return Err(SolidError::query_failed(
            OP,
            "the curve has no closest point",
        ));
    }
    solutions
        .as_slice()
        .iter()
        .map(|s| curve_result(k, curve, s))
        .collect()
}

/// Closest surface point and UV representations (`SmApiSurfaceClosestPoint`).
/// Strictly smaller distance replaces the selected solution; failed secondary
/// evaluations are ignored. UVs must evaluate within the surface zone tolerance.
pub fn surface_closest_point(
    k: &Kernel,
    surface: &Surface,
    point: Point3,
) -> Result<SurfaceClosestPoint> {
    let drop = surface.drop_point::<NoObject>(
        k,
        point,
        &surface.natural_uv_domain_with_nurb(k)?,
        &DropPointOptions {
            want_solutions: true,
            ..DropPointOptions::default()
        },
    )?;
    if !drop.success || drop.solutions.is_empty() {
        return Err(SolidError::query_failed(
            OP,
            "the point does not drop onto the surface",
        ));
    }
    let tolerance = zone_tol_3d(k, surface)?;
    let mut best =
        drop.solutions.as_slice().first().ok_or_else(|| {
            SolidError::query_failed(OP, "the point does not drop onto the surface")
        })?;
    for solution in drop.solutions.as_slice().iter().skip(1) {
        if solution.start.solution_value < best.start.solution_value {
            best = solution;
        }
    }
    let selected = Point2::new(best.start.param(0)?, best.start.param(1)?);
    let point = surface.evaluate_point(k, selected)?;
    let mut uvs = Vec::new();
    for solution in drop.solutions.as_slice() {
        let uv = Point2::new(solution.start.param(0)?, solution.start.param(1)?);
        if let Some(candidate) = ignored(surface.evaluate_point(k, uv))? {
            if candidate.closer_than(tolerance, point) {
                uvs.push(uv);
            }
        }
    }
    if uvs.is_empty() {
        return Err(SolidError::query_failed(
            OP,
            "no surface solution has a parameter",
        ));
    }
    Ok(SurfaceClosestPoint {
        closest: ClosestPoint {
            point,
            distance: best.start.solution_value,
        },
        uvs,
    })
}

fn solve_options(_k: &Kernel, requested: SolutionRequested) -> TopologySolveOptions<'static> {
    TopologySolveOptions {
        operation: SolverOperation::Minimize,
        requested,
        tolerance_3d: crate::geom::tol::ZONE_TOL_3D,
        best_answer_so_far_sq: 1.0e20 * 1.0e20,
        opt_vectors: &[],
    }
}

// SmSolution::GetPoint using the same caller-owned geometry adapter as the solver.
fn solution_point<O: SolutionObject>(
    k: &Kernel,
    objects: &dyn TopologyObjects<O>,
    solution: &Solution<O>,
    index: usize,
) -> Result<Point3> {
    let mut parameter = 0;
    for (i, object) in solution
        .objects
        .iter()
        .take(solution.num_objects)
        .enumerate()
    {
        let object =
            object.ok_or_else(|| SolidError::query_missing(OP, "the solution's object"))?;
        let kind = object.kind();
        if i == index {
            if kind == ObjectKind::Vertex {
                return objects.vertex_point(object);
            }
            if matches!(kind, ObjectKind::Curve | ObjectKind::Edge) {
                let curve = if kind == ObjectKind::Edge {
                    objects
                        .edge_curve(object)?
                        .ok_or_else(|| SolidError::query_missing(OP, "the edge's curve"))?
                } else {
                    object
                };
                return Ok(ignored(
                    objects
                        .curve(curve)?
                        .evaluate_point(k, solution.start.param(parameter)?),
                )?
                .unwrap_or(Point3::UNDEF));
            }
            if matches!(kind, ObjectKind::Surface | ObjectKind::Face) {
                let surface = if kind == ObjectKind::Face {
                    objects.face_surface(object)?
                } else {
                    object
                };
                return Ok(ignored(objects.surface(surface)?.evaluate_point(
                    k,
                    Point2::new(
                        solution.start.param(parameter)?,
                        solution.start.param(parameter + 1)?,
                    ),
                ))?
                .unwrap_or(Point3::UNDEF));
            }
            return Err(SolidError::query_failed(
                OP,
                "the solution's object is not a vertex, curve or edge",
            ));
        }
        parameter += match kind {
            ObjectKind::Curve | ObjectKind::Edge => 1,
            ObjectKind::Face => 2,
            _ => 0,
        };
    }
    Err(SolidError::query_failed(
        OP,
        "the solution has no object at the index",
    ))
}

/// Closest point on a B-rep (`SmApiGetClosestPoint`); the adapter supplies its cache.
/// Solver errors, point-extraction failures and empty solution sets propagate.
pub fn get_closest_point<O: SolutionObject>(
    k: &Kernel,
    objects: &dyn TopologyObjects<O>,
    brep: O,
    point: Point3,
) -> Result<ClosestPoint> {
    brep_closest_point(k, objects, brep, point)
}

/// Single closest B-rep point (`SmApiBrepClosestPoint`).
/// Errors are those of `BrepPointSolve` and solution point extraction.
pub fn brep_closest_point<O: SolutionObject>(
    k: &Kernel,
    objects: &dyn TopologyObjects<O>,
    brep: O,
    point: Point3,
) -> Result<ClosestPoint> {
    let solutions = TopologySolver::brep_point_solve(
        k,
        objects,
        Some(brep),
        point,
        solve_options(k, SolutionRequested::Single),
    )?;
    let solution = solutions
        .as_slice()
        .first()
        .ok_or_else(|| SolidError::query_failed(OP, "the B-rep has no closest point"))?;
    Ok(ClosestPoint {
        point: solution_point(k, objects, solution, 0)?,
        distance: solution.start.solution_value,
    })
}

/// All closest B-rep points in solver order (`SmApiBrepClosestPointAll`).
/// An empty solution set is `SM_ERR`; extraction failures propagate.
pub fn brep_closest_point_all<O: SolutionObject>(
    k: &Kernel,
    objects: &dyn TopologyObjects<O>,
    brep: O,
    point: Point3,
) -> Result<Vec<ClosestPoint>> {
    let solutions = TopologySolver::brep_point_solve(
        k,
        objects,
        Some(brep),
        point,
        solve_options(k, SolutionRequested::All),
    )?;
    if solutions.is_empty() {
        return Err(SolidError::query_failed(
            OP,
            "the B-rep has no closest point",
        ));
    }
    solutions
        .as_slice()
        .iter()
        .map(|s| {
            Ok(ClosestPoint {
                point: solution_point(k, objects, s, 0)?,
                distance: s.start.solution_value,
            })
        })
        .collect()
}

/// B-rep arena and its handle in the caller's topology-solver adapter.
#[derive(Clone, Copy)]
pub struct BrepRef<'a, O> {
    /// Exact topology.
    pub brep: &'a Brep,
    /// Adapter handle.
    pub handle: O,
}

/// Relationship code and boundary distance (`SmApiBrepRelationship`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrepRelationship {
    /// 0 separate, 1 touching, 2 interpenetrating, 3 A contains B, 4 B contains A.
    pub relationship: i32,
    /// Boundary gap, or zero when within the sum of the B-rep tolerances.
    pub distance: f64,
}

/// A B-rep points are classified against (`pTarget`).
struct Target<'b>(BrepClassifier<'static, 'b>);

impl<'b> Target<'b> {
    fn new(brep: &'b Brep) -> Self {
        Self(BrepClassifier::of_brep(brep))
    }

    /// `pTarget->Point3DClassify(point, tolerance, flag, sPC)`: the point class and
    /// whether its object is a region that is not void (`pReg != NULL &&
    /// !pReg->IsVoid()`).
    fn classify(
        &mut self,
        k: &Kernel,
        point: Point3,
        tolerance: f64,
        flag: bool,
    ) -> Result<(PointClassType, bool)> {
        let pc = self.0.point_3d_classify(k, point, tolerance, flag)?;
        let material = match pc.object() {
            Some(EntityRef::Region(r)) => self.0.brep().region(r)?.is_void() == 0,
            _ => false,
        };
        Ok((pc.point_class(), material))
    }
}

/// `sm_PointInSolid`: whether `point` classifies to a material region of `target`
/// (a failing classification is `false`).
fn point_in_solid(
    k: &Kernel,
    target: &mut Target<'_>,
    point: Point3,
    tolerance: f64,
) -> Result<bool> {
    Ok(ignored(target.classify(k, point, tolerance, false))?
        .is_some_and(|(class, material)| class == PointClassType::Region && material))
}

fn brep_vertices(brep: &Brep) -> Result<Vec<VertexId>> {
    crate::topology::brep::p1::vertices(brep, None)
}

/// `SmTopologySolver::BrepCurveSolve(pTarget, curve, interval, SM_SO_INTERSECT,
/// SM_SR_ALL, tolerance, SM_BIG_DOUBLE, NULL)` through the caller's objects: every
/// solution's start parameter, and its end parameter for a range of values, in solution
/// order.
fn target_curve_crossings<O: SolutionObject>(
    k: &Kernel,
    objects: &dyn TopologyObjects<O>,
    target: O,
    curve: CurveRef<'_, O>,
    interval: &Extent1d,
    tolerance: f64,
) -> Result<Vec<f64>> {
    let solutions = TopologySolver::brep_curve_solve(
        k,
        objects,
        Some(target),
        curve,
        interval,
        TopologySolveOptions {
            operation: SolverOperation::Intersect,
            requested: SolutionRequested::All,
            tolerance_3d: tolerance,
            best_answer_so_far_sq: crate::math::BIG_DOUBLE,
            opt_vectors: &[],
        },
    )?;
    let mut params = Vec::new();
    for solution in solutions.as_slice() {
        params.push(solution.start.param(0)?);
        if solution.solution_type == SolutionType::RangeOfValues {
            params.push(solution.end.param(0)?);
        }
    }
    Ok(params)
}

/// One point on each piece of each edge of `sample` between its crossings of
/// `target`'s boundary (`sm_CountContainedSamples`' edge loop over `GetEdges`). The edges
/// come from the sample's curve tree (one object per edge curve; edges without a curve
/// are skipped, as upstream), so their order is the tree's: only the inside/outside
/// counts of the points are used.
fn edge_piece_points<O: SolutionObject>(
    k: &Kernel,
    objects: &dyn TopologyObjects<O>,
    sample: O,
    target: O,
    tolerance: f64,
) -> Result<Vec<Point3>> {
    let mut curve_objects: Vec<O> = Vec::new();
    {
        let trees = objects.brep_trees(sample)?;
        for item in trees.curves.object_list()? {
            if let Some(o) = trees.curves.list(item)?.object {
                if !curve_objects.contains(&o) {
                    curve_objects.push(o);
                }
            }
        }
    }
    let mut points = Vec::new();
    for handle in curve_objects {
        let Some(edge) = objects.curve_edge(handle)? else {
            continue;
        };
        let interval = objects.edge_interval(edge)?;
        let curve = objects.curve(handle)?;
        let mut params = vec![interval.min()];
        if let Some(crossings) = ignored(target_curve_crossings(
            k,
            objects,
            target,
            CurveRef { curve, handle },
            &interval,
            tolerance,
        ))? {
            params.extend(crossings);
        }
        params.push(interval.max());
        params.sort_by(f64::total_cmp);
        for pair in params.windows(2) {
            let &[a, b] = pair else {
                continue;
            };
            if b - a <= EFF_ZERO * (1.0 + interval.length()) {
                continue;
            }
            if let Some(mid) = ignored(curve.evaluate_point(k, 0.5 * (a + b)))? {
                points.push(mid);
            }
        }
    }
    Ok(points)
}

fn count_contained_samples<O: SolutionObject>(
    k: &Kernel,
    objects: &dyn TopologyObjects<O>,
    sample: &Brep,
    handles: [O; 2],
    target: &mut Target<'_>,
    tolerance: f64,
) -> Result<(usize, usize)> {
    let [sample_handle, target_handle] = handles;
    let mut points = Vec::new();
    for vertex in brep_vertices(sample)? {
        points.push(sample.vertex(vertex)?.point);
    }
    for face in brep_faces(sample)? {
        // SmApiFaceInternalPoint on a sample borrowed next to the classified target: on a
        // copy of the sample (module Deviations)
        if let Some(point) = ignored(face::calculate_an_internal_point_snapshot(k, sample, face))? {
            points.push(point);
        }
    }

    // One point on each piece of each edge between its crossings of the target's
    // boundary: each piece is wholly inside or outside the target.
    points.extend(edge_piece_points(
        k,
        objects,
        sample_handle,
        target_handle,
        tolerance,
    )?);

    let (mut inside, mut outside) = (0, 0);
    for point in points {
        if let Some((class, material)) = ignored(target.classify(k, point, tolerance, true))? {
            match class {
                PointClassType::Region => {
                    if material {
                        inside += 1;
                    } else {
                        outside += 1;
                    }
                }
                PointClassType::Unknown => outside += 1,
                _ => {}
            }
        }
    }
    Ok((inside, outside))
}

/// Boundary distance followed by containment/touching classification.
/// Missing point-classification/list dependencies return `Unported` rather than
/// being swallowed by upstream's ignored ordinary classification statuses.
pub fn brep_relationship<O: SolutionObject>(
    k: &Kernel,
    objects: &dyn TopologyObjects<O>,
    a: BrepRef<'_, O>,
    b: BrepRef<'_, O>,
) -> Result<BrepRelationship> {
    let solution = brep_relationship_solve(k, objects, a.handle, b.handle)?;
    brep_relationship_classify(k, objects, &solution, [a.handle, b.handle], a.brep, b.brep)
}

/// The boundary distance solve of [`brep_relationship`] (`BrepBrepSolve`, single
/// solution) on the B-reps behind `a` and `b`. A caller whose `objects` hold the
/// B-reps mutably (face caches made on demand) runs it, then
/// [`brep_relationship_classify`] on the B-reps.
///
/// # Errors
///
/// Those of the solver; `SM_ERR` without a solution.
pub fn brep_relationship_solve<O: SolutionObject>(
    k: &Kernel,
    objects: &dyn TopologyObjects<O>,
    a: O,
    b: O,
) -> Result<Solution<O>> {
    let solutions = TopologySolver::brep_brep_solve(
        k,
        objects,
        Some(a),
        Some(b),
        solve_options(k, SolutionRequested::Single),
    )?;
    solutions
        .as_slice()
        .first()
        .copied()
        .ok_or_else(|| SolidError::query_failed(OP, "the B-reps have no solution"))
}

/// The containment and touching classification of [`brep_relationship`] after its
/// solve ([`brep_relationship_solve`]), `brep_a` and `brep_b` being the B-reps the
/// solution's objects name and `handles` their handles in `objects` (the edge-piece
/// samples solve each sample edge against the other B-rep through them).
///
/// # Errors
///
/// Those of the solution points and the point classifications.
pub fn brep_relationship_classify<O: SolutionObject>(
    k: &Kernel,
    objects: &dyn TopologyObjects<O>,
    solution: &Solution<O>,
    handles: [O; 2],
    brep_a: &Brep,
    brep_b: &Brep,
) -> Result<BrepRelationship> {
    let [ha, hb] = handles;
    let a = BrepRef {
        brep: brep_a,
        handle: (),
    };
    let b = BrepRef {
        brep: brep_b,
        handle: (),
    };
    let distance = solution.start.solution_value;
    let tolerance = crate::geom::tol::context_zone_tol_3d(k, None, SmallTopoType::NormalSize);
    let (mut target_a, mut target_b) = (Target::new(a.brep), Target::new(b.brep));
    if distance > a.brep.tolerance() + b.brep.tolerance() {
        let pa = ignored(solution_point(k, objects, solution, 0))?;
        let pb = if pa.is_some() {
            ignored(solution_point(k, objects, solution, 1))?
        } else {
            None
        };
        let relationship = if let (Some(pa), Some(pb)) = (pa, pb) {
            let bin_a = point_in_solid(k, &mut target_a, pb, tolerance)?;
            let ain_b = point_in_solid(k, &mut target_b, pa, tolerance)?;
            if bin_a {
                3
            } else if ain_b {
                4
            } else {
                0
            }
        } else {
            let (bin, bout) =
                count_contained_samples(k, objects, b.brep, [hb, ha], &mut target_a, tolerance)?;
            let (ain, aout) =
                count_contained_samples(k, objects, a.brep, [ha, hb], &mut target_b, tolerance)?;
            if bin > 0 && bout == 0 {
                3
            } else if ain > 0 && aout == 0 {
                4
            } else {
                0
            }
        };
        Ok(BrepRelationship {
            relationship,
            distance,
        })
    } else {
        let (bin, _) =
            count_contained_samples(k, objects, b.brep, [hb, ha], &mut target_a, tolerance)?;
        let (ain, _) =
            count_contained_samples(k, objects, a.brep, [ha, hb], &mut target_b, tolerance)?;
        Ok(BrepRelationship {
            relationship: if bin > 0 || ain > 0 { 2 } else { 1 },
            distance: 0.0,
        })
    }
}

/// Boundary distance through the same relationship query (`SmApiBrepDistance`).
/// Containment/classification errors propagate, even though only distance is returned.
pub fn brep_distance<O: SolutionObject>(
    k: &Kernel,
    objects: &dyn TopologyObjects<O>,
    a: BrepRef<'_, O>,
    b: BrepRef<'_, O>,
) -> Result<f64> {
    Ok(brep_relationship(k, objects, a, b)?.distance)
}

fn brep_edges(brep: &Brep) -> Result<Vec<EdgeId>> {
    crate::topology::brep::p1::edges(brep, None)
}
fn brep_faces(brep: &Brep) -> Result<Vec<FaceId>> {
    crate::topology::brep::p1::faces(brep, None)
}

/// Exact B-rep edge list (`SmApiGetEdges`). Empty lists return `SM_ERR`.
/// Missing list traversal is `Unported`.
pub fn get_edges(brep: &Brep) -> Result<Vec<EdgeId>> {
    let edges = brep_edges(brep)?;
    if edges.is_empty() {
        return Err(SolidError::query_failed(OP, "the B-rep has no edges"));
    }
    Ok(edges)
}

/// Exact B-rep face list (`SmApiGetFaces`). Empty lists return `SM_ERR`.
/// Missing list traversal is `Unported`.
pub fn get_faces(brep: &Brep) -> Result<Vec<FaceId>> {
    let faces = brep_faces(brep)?;
    if faces.is_empty() {
        return Err(SolidError::query_failed(OP, "the B-rep has no faces"));
    }
    Ok(faces)
}

/// Flattened edges and orientations in upward face-use loop order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryLoops {
    /// Number of edge uses in each loop (including zero for a vertex loop).
    pub edge_counts: Vec<usize>,
    /// Edge of each edge use, retaining repeated edges.
    pub edges: Vec<EdgeId>,
    /// Same or opposite orientation of each edge use.
    pub orientations: Vec<OrientType>,
}

/// Upward boundary loops (`SmApiFaceGetBoundaryLoops`).
/// Missing face use/loops and invalid edge/orientation return `SM_ERR`.
pub fn face_get_boundary_loops(brep: &Brep, id: FaceId) -> Result<BoundaryLoops> {
    let mut result = BoundaryLoops {
        edge_counts: Vec::new(),
        edges: Vec::new(),
        orientations: Vec::new(),
    };
    let loops = if let Some(fu) = face::upward_faceuse(brep, id)? {
        eu::faceuse_loopuses(brep, fu, false, None)?
    } else {
        Vec::new()
    };
    if loops.is_empty() {
        return Err(SolidError::query_failed(
            OP,
            "the face has no boundary loops",
        ));
    }
    for lu in loops {
        let edges = eu::loopuse_edgeuses(brep, lu)?;
        result.edge_counts.push(edges.len());
        for edgeuse in edges {
            let edgeuse = brep.edgeuse(edgeuse)?;
            let edge = edgeuse
                .edge
                .ok_or_else(|| SolidError::query_failed(OP, "the edge use has no edge"))?;
            if !matches!(edgeuse.orientation, OrientType::Same | OrientType::Opposite) {
                return Err(SolidError::query_failed(
                    OP,
                    "the edge use's orientation is neither same nor opposite",
                ));
            }
            result.edges.push(edge);
            result.orientations.push(edgeuse.orientation);
        }
    }
    Ok(result)
}

/// Finds an edge index and parameter (`SmApiFindEdge`); missing match is `SM_ERR`.
pub fn find_edge(k: &Kernel, brep: &Brep, point: Point3) -> Result<EdgePick> {
    tools::find_edge(k, brep, point)?
        .ok_or_else(|| SolidError::query_failed(OP, "no edge is at the point"))
}

/// Finds a face (`SmApiFindFaces`); missing match is `SM_ERR`.
pub fn find_faces(
    k: &Kernel,
    brep: &mut Brep,
    id: BrepId,
    point: Point3,
    keys: &dyn Fn(CacheObject) -> ObjectRef,
    owners: &dyn Fn(ObjectRef) -> Option<ObjectRef>,
) -> Result<FaceId> {
    tools::find_face(k, brep, id, point, keys, owners)?
        .ok_or_else(|| SolidError::query_failed(OP, "no face is at the point"))
}

/// Manifold solid predicate (`SmApiBrepIsManifoldSolid`: `SmBrep::IsManifoldSolid`).
pub fn brep_is_manifold_solid(brep: &Brep) -> Result<bool> {
    crate::topology::brep::p2::is_manifold_solid(brep)
}

/// (Not upstream) Whether `brep` is closed and manifold: no shell vertex and only manifold
/// edges, [`brep_is_manifold_solid`] without its test that the regions number at least two.
pub fn brep_is_closed_manifold(brep: &Brep) -> Result<bool> {
    crate::topology::brep::p2::is_closed_manifold(brep)
}

/// Whether `brep` is a closed body whose solids touch only along edges or at vertices
/// (not upstream: the result test of a boolean of solids that may touch): at least two
/// regions, no shell vertex, and every edge closed, neither a wire nor bounding a face
/// on only one side: its edge uses come four to each face round it (two faces, each used
/// on both sides), so a count that is a positive multiple of four.
///
/// # Errors
///
/// Those of the topology queries.
pub fn brep_is_closed_touching_solids(brep: &Brep) -> Result<bool> {
    crate::topology::brep::p2::is_closed_where(brep, |b, e| {
        if edge::is_wire(b, e)? {
            return Ok(false);
        }
        let n = edge::edgeuses(b, e, None, None)?.len();
        // (a multiple of four: its two lowest bits clear)
        Ok(n >= 4 && n.trailing_zeros() >= 2)
    })
}

/// Counts a B-rep's material regions and enclosed void cavities
/// (`SmApiBrepMaterialCensus`), returned as `(solids, voids)`.
///
/// One walk of `SmBrep::GetRegions`: a region whose `IsVoid()` is nonzero (`TRUE` or
/// `UNSURE`) counts as a void unless it is the infinite region; any other region counts
/// as a solid. A null B-rep (`SM_ERR_INVALID_INPUT`) has no typed equivalent.
///
/// # Errors
///
/// A dangling region on the region list.
pub fn brep_material_census(brep: &Brep) -> Result<(usize, usize)> {
    let mut solids = 0usize;
    let mut voids = 0usize;
    for id in crate::topology::brep::p1::regions(brep, None)? {
        let region = brep.region(id)?;
        if region.is_void() != 0 {
            if !region.is_infinite_region(brep, id) {
                voids += 1;
            }
        } else {
            solids += 1;
        }
    }
    Ok((solids, voids))
}

/// Loose or tight B-rep box (`SmApiBrepBoundingBox`); missing queries are `Unported`.
pub fn brep_bounding_box(k: &Kernel, brep: &Brep, tight: bool) -> Result<Extent3d> {
    if tight {
        crate::query::brep::calculate_tight_bounding_box(k, brep)
    } else {
        qbrep::calculate_bounding_box(k, brep, BoundingBoxOptions::default())
    }
}

/// Loose or tight face box (`SmApiFaceBoundingBox`); missing queries are `Unported`.
pub fn face_bounding_box(k: &Kernel, brep: &Brep, face: FaceId, tight: bool) -> Result<Extent3d> {
    if tight {
        brep.face(face)?;
        return crate::query::face::p3::calculate_tight_bounding_box(k, brep, face);
    }
    crate::query::face::p1::calculate_bounding_box(k, brep, face, false)
}

/// Interior face point (`SmApiFaceInternalPoint`). The face's line classifications run
/// on `brep` in place, so the lazy edits upstream's `const` query makes (UV trim
/// curves, gap caches, tolerance growth) stay on it. Errors from the face query
/// propagate, including explicit deferrals of its unfinished dependencies.
pub fn face_internal_point(k: &Kernel, brep: &mut Brep, face: FaceId) -> Result<Point3> {
    face::calculate_an_internal_point(k, brep, face)
}

/// Trim-aware UV classification (`SmApiFaceClassifyUV`), preserving the face
/// classifier's vertex tests, loop containment and default classification tolerance.
pub fn face_classify_uv(
    k: &Kernel,
    brep: &mut Brep,
    face: FaceId,
    uv: Point2,
) -> Result<PointClassType> {
    let source_tolerance = brep.face(face)?.tolerance;
    let tolerance = crate::geom::tol::zone_tol3d(k, crate::geom::tol::USE_DEFAULT);
    let mut classification = PointClassification::new(tolerance);
    crate::query::face::p2::point_classify(
        k,
        brep,
        face,
        uv,
        &mut classification,
        crate::query::face::p2::PointClassifyOptions {
            src_zone_tol_3d: source_tolerance,
            use_uv_classification: true,
            ..crate::query::face::p2::PointClassifyOptions::default()
        },
    )?;
    Ok(classification.point_class())
}

/// Trim-aware closest point on one face (`SmApiFaceClosestPoint`), the face's B-rep
/// held in `breps`. Shape construction and solver errors propagate.
pub fn face_closest_point<O: SolutionObject>(
    k: &mut Kernel,
    breps: &dyn BrepStore,
    objects: &dyn TopologyObjects<O>,
    face: FaceKey,
    point: Point3,
) -> Result<ClosestPoint> {
    let shape = Shape::new(k, breps, &[], &[face], &[], &[])?;
    face_closest_point_in_shape(k, &shape, objects, face, point)
}

/// [`face_closest_point`] on the face's shape already made (`SmShape` of the face):
/// the solve may then read the face's B-rep through `objects` mutably (face caches
/// and UV trim curves made on demand).
///
/// # Errors
///
/// Those of the solver and of the solution point.
pub fn face_closest_point_in_shape<O: SolutionObject>(
    k: &Kernel,
    shape: &Shape,
    objects: &dyn TopologyObjects<O>,
    face: FaceKey,
    point: Point3,
) -> Result<ClosestPoint> {
    let mut options = solve_options(k, SolutionRequested::Single);
    options.tolerance_3d =
        objects.face_tolerance(objects.shape_member_object(ShapeMember::Face(face))?)?;
    let solutions = TopologySolver::shape_point_solve(k, objects, shape, point, options)?;
    let solution = solutions
        .as_slice()
        .first()
        .ok_or_else(|| SolidError::query_failed(OP, "the face has no closest point"))?;
    Ok(ClosestPoint {
        point: solution_point(k, objects, solution, 0)?,
        distance: solution.start.solution_value,
    })
}

/// Axis-aligned edge box (`SmApiEdgeBoundingBox`), preserving the distinction
/// between the direct tight-box call and loose-box calculation.
pub fn edge_bounding_box(k: &Kernel, brep: &Brep, edge: EdgeId, tight: bool) -> Result<Extent3d> {
    if tight {
        return Ok(
            crate::query::edge::calculate_tight_bounding_box(k, brep, edge)?.unwrap_or_default(),
        );
    }
    crate::query::edge::calculate_bounding_box(
        k,
        brep,
        edge,
        BoundingBoxRequest {
            normal: true,
            pseudo: false,
            polar: false,
        },
        false,
    )?
    .normal
    .ok_or_else(|| SolidError::query_failed(OP, "the face has no normal there"))
}

/// Closest point within an edge interval (`SmApiEdgeClosestPoint`).
/// A failed drop or absent curve returns `SM_ERR`; evaluation errors propagate.
pub fn edge_closest_point(
    k: &Kernel,
    brep: &Brep,
    id: EdgeId,
    point: Point3,
) -> Result<CurveClosestPoint> {
    let result = crate::query::edge::closest_point(k, brep, id, point)?
        .ok_or_else(|| SolidError::query_failed(OP, "the point does not drop onto the edge"))?;
    let curve = brep
        .edge(id)?
        .curve
        .as_ref()
        .ok_or_else(|| SolidError::query_failed(OP, "the edge has no curve"))?;
    Ok(CurveClosestPoint {
        closest: ClosestPoint {
            point: curve.evaluate_point(k, result.parameter)?,
            distance: result.distance,
        },
        parameter: result.parameter,
    })
}

/// Parameter interval (`SmApiEdgeParameterRange`); dangling ids are errors.
pub fn edge_parameter_range(brep: &Brep, id: EdgeId) -> Result<Extent1d> {
    Ok(brep.edge(id)?.interval)
}

/// Classifies a parameter (`SmApiEdgeClassifyParameter`). Vertices are tested
/// first with strict distance comparisons, including parameters outside the edge
/// interval. An ordinary evaluation failure falls through to interval containment.
pub fn edge_classify_parameter(
    k: &Kernel,
    brep: &Brep,
    id: EdgeId,
    parameter: f64,
) -> Result<PointClassType> {
    let e = brep.edge(id)?;
    let curve = e
        .curve
        .as_ref()
        .ok_or_else(|| SolidError::query_failed(OP, "the edge has no curve"))?;
    if let Some(point) = ignored(curve.evaluate_point(k, parameter))? {
        for v in [edge::start_vertex(brep, id)?, edge::end_vertex(brep, id)?]
            .into_iter()
            .flatten()
        {
            let tolerance = crate::topology::vertex::tolerance(brep, v)?;
            if point.distance(brep.vertex(v)?.point) < tolerance {
                return Ok(PointClassType::Vertex);
            }
        }
    }
    Ok(if e.interval.contains_value(parameter, 0.0) {
        PointClassType::Edge
    } else {
        PointClassType::Unknown
    })
}

/// Unit tangent (`SmApiEdgeTangent`), evaluating from the left as upstream.
/// A missing curve, evaluation failure or zero derivative returns an error.
pub fn edge_tangent(k: &Kernel, brep: &Brep, id: EdgeId, parameter: f64) -> Result<Vec3> {
    let curve = brep
        .edge(id)?
        .curve
        .as_ref()
        .ok_or_else(|| SolidError::query_failed(OP, "the edge has no curve"))?;
    let values = curve.evaluate(k, parameter, 1, true, true)?;
    values
        .get(1)
        .copied()
        .ok_or_else(|| SolidError::query_failed(OP, "the curve has no first derivative there"))?
        .unitize()
}

/// Vertex position (`SmApiVertexGetPoint`); dangling ids are errors.
pub fn vertex_get_point(brep: &Brep, id: VertexId) -> Result<Point3> {
    Ok(brep.vertex(id)?.point)
}

/// A distance with a proved bound on its rounding: the exact distance between the
/// stated points is in `[value - bound, value + bound]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Distance {
    /// The distance as rounded, bit for bit `Vec3::distance` of the same points.
    pub value: f64,
    /// A proved bound on `|value - exact|`, finite and not negative.
    pub bound: f64,
}

/// The distance between `a` and `b` with a proved bound on its rounding (not
/// upstream). The value is `a.distance(b)` bit for bit; the bound comes from the same
/// sum of squares in rounded interval arithmetic
/// ([`crate::blend::fillet_section::interval`]), so it holds below the normal range
/// too, where a squared difference underflows.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a non-finite coordinate, or a distance that
/// overflows.
pub fn point_distance(a: Point3, b: Point3) -> Result<Distance> {
    use crate::blend::fillet_section::interval::{cmax, round_up, Ivl};
    let finite = |p: Point3| p.x.is_finite() && p.y.is_finite() && p.z.is_finite();
    if !finite(a) || !finite(b) {
        return Err(SolidError::invalid_input(OP, "non-finite point"));
    }
    let value = a.distance(b);
    let square = |x: f64, y: f64| {
        let d = Ivl::from(x) - Ivl::from(y);
        d * d
    };
    let exact = (square(a.x, b.x) + square(a.y, b.y) + square(a.z, b.z)).sqrt();
    // each difference rounded up: the bound is at least each end's distance from value
    let bound = cmax(round_up(exact.hi - value), round_up(value - exact.lo));
    if !value.is_finite() || !bound.is_finite() {
        return Err(SolidError::invalid_input(OP, "distance overflows"));
    }
    Ok(Distance {
        value,
        bound: cmax(bound, 0.0),
    })
}

/// The distance from vertex `id`'s point to `point`, as [`point_distance`] (not
/// upstream; the point picks' vertex distance).
///
/// # Errors
///
/// Those of [`point_distance`]; [`SolidError::NotFound`] for a dangling id.
pub fn vertex_distance(brep: &Brep, id: VertexId, point: Point3) -> Result<Distance> {
    point_distance(brep.vertex(id)?.point, point)
}

/// [`edgeuse_face_frame`]'s result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeuseFaceFrame {
    /// The surface parameters of the edge use's point (its UV trim curve's), where
    /// the normal is taken.
    pub uv: Point2,
    /// The face's unit normal there, turned by the faceuse's sense, with its proved
    /// bound ([`crate::query::normal`]).
    pub normal: UnitVector,
}

/// The unit normal, at the edge parameter `parameter`, of the face edge use `eu`
/// bounds, in the sense of `faceuse` (one of that face's two uses): the surface normal
/// at the edge use's point on the surface ([`evaluate_binormal`]'s (u, v)), unitised,
/// negated where `faceuse` is opposite to the surface, and certified against the exact
/// normal there in `faceuse`'s sense ([`crate::query::normal::certify`] with
/// [`Sense::Reversed`] for a use opposite to the surface: the sense is the use's
/// orientation, proved, never taken from the value). Not upstream (the point picks' and
/// chamfer sides' face normal at an edge). The binormal is not returned: it is not yet
/// certified.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for an edge use bounding no face, a `faceuse` of another
/// face, a face without a surface, or a zero normal; [`SolidError::Unsupported`] for a
/// `faceuse` whose orientation is `Unknown` (no sense to prove); those of [`evaluate_binormal`],
/// the surface normal and [`crate::query::normal::certify`].
pub fn edgeuse_face_frame(
    k: &Kernel,
    brep: &mut Brep,
    eu: EdgeuseId,
    parameter: f64,
    faceuse: FaceuseId,
) -> Result<EdgeuseFaceFrame> {
    let f = eu::face(brep, eu)?
        .ok_or_else(|| SolidError::invalid_input(OP, "edge use bounds no face"))?;
    let use_ = brep.faceuse(faceuse)?;
    if use_.face != Some(f) {
        return Err(SolidError::invalid_input(OP, "a faceuse of another face"));
    }
    let (sign, sense) = match use_.orientation {
        OrientType::Same => (1.0, Sense::Surface),
        OrientType::Opposite => (-1.0, Sense::Reversed),
        OrientType::Unknown => {
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: "a faceuse whose orientation is unknown has no proved sense",
            })
        }
    };
    let at = evaluate_binormal(k, brep, eu, parameter, false, &BinormalOptions::default())?;
    let surface = brep
        .face(f)?
        .surface
        .as_ref()
        .ok_or_else(|| SolidError::invalid_input(OP, "face without a surface"))?;
    let n = qbrep::evaluate_surface_normal(k, Some(surface), at.uv)?;
    let len = n.length();
    if !(len > 0.0 && len.is_finite()) {
        return Err(SolidError::invalid_input(OP, "zero normal"));
    }
    Ok(EdgeuseFaceFrame {
        uv: at.uv,
        normal: certify_normal(k, surface, at.uv, n * (sign / len), sense)?,
    })
}

/// [`edgeuse_face_frame`] of face `f`'s use of edge `e`, at the middle of the edge's
/// interval, in the sense of `faceuse` (not upstream: the parameter an application's
/// face normal at an edge is taken at).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for an edge that does not bound `f` or an edge interval
/// that is not finite; those of [`edgeuse_face_frame`].
pub fn face_normal_at_edge(
    k: &Kernel,
    brep: &mut Brep,
    e: EdgeId,
    f: FaceId,
    faceuse: FaceuseId,
) -> Result<EdgeuseFaceFrame> {
    let eu = edge::edgeuse_of_face(brep, e, Some(f))?
        .ok_or_else(|| SolidError::invalid_input(OP, "the edge does not bound the face"))?;
    let ivl = brep.edge(e)?.interval;
    let t = 0.5 * (ivl.min() + ivl.max());
    if !t.is_finite() {
        return Err(SolidError::invalid_input(OP, "edge interval not finite"));
    }
    edgeuse_face_frame(k, brep, eu, t, faceuse)
}

/// One sample of [`edge_face_gap_samples`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeFaceGapSample {
    /// The edge curve's parameter.
    pub edge_parameter: f64,
    /// The curve's point there, as evaluated.
    pub edge_point: Point3,
    /// The surface's (u, v) the drop found.
    pub face_uv: Point2,
    /// The surface's point there, as evaluated (rounded: not a proved point of S).
    pub face_point: Point3,
    /// The distance between the two evaluated points, with its proved rounding bound.
    pub distance: Distance,
    /// A proved upper bound on the distance from `edge_point` to the surface S
    /// ([`crate::query::normal`]): the largest distance from `edge_point` to the
    /// enclosure of the exact S(`face_uv`). `None` where S(`face_uv`) is not enclosed: a
    /// kind [`crate::query::normal::surface_point`] refuses, or a point where it does not
    /// prove the net's weight positive (its `Degenerate`); the sample is then a sample
    /// only, and the other samples keep their bounds.
    pub surface_bound: Option<f64>,
}

/// The samples of SMLib's edge/face gap function (`SmCrvSrfGapFunction` with 20
/// samples over the edge interval, as `SmEdgeuse::CalcMaxEdgeFaceGap` builds it) for
/// edge `e`'s curve and face `f`'s surface, over that surface's own domain; the edge
/// and face need not be adjacent (not upstream as a query).
///
/// What is proved, per sample: `distance` is the distance between the two evaluated
/// points within its bound; `surface_bound`, where present, is an upper bound on the
/// distance from `edge_point` to the surface, from the enclosure of the exact
/// S(`face_uv`) (the evaluated `face_point` is rounded, so `distance` alone bounds
/// nothing about S). What is not proved: that the drop found the nearest surface point
/// (the sample may overestimate the gap), that `edge_point` is the curve's exact point,
/// or that the largest sample is the largest gap along the edge (the curve is sampled,
/// not bounded between the samples).
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::InvalidInput`] for an
/// edge without a curve, a face without a surface, or an empty interval; those of
/// [`GapFunction::build_sample_set`], [`point_distance`] and
/// [`crate::query::normal::surface_point`] other than its refusal of a kind or of a point
/// (those make a sample's `surface_bound` `None`).
pub fn edge_face_gap_samples(
    k: &Kernel,
    brep: &Brep,
    e: EdgeId,
    f: FaceId,
) -> Result<Vec<EdgeFaceGapSample>> {
    let interval = brep.edge(e)?.interval;
    let curve = brep
        .edge(e)?
        .curve
        .as_ref()
        .ok_or_else(|| SolidError::invalid_input(OP, "edge without a curve"))?;
    let surface = brep
        .face(f)?
        .surface
        .as_ref()
        .ok_or_else(|| SolidError::invalid_input(OP, "face without a surface"))?;
    if interval.max().partial_cmp(&interval.min()) != Some(core::cmp::Ordering::Greater) {
        return Err(SolidError::invalid_input(OP, "empty edge interval"));
    }
    // The tolerance only classifies the samples (in or out of tolerance); it changes no
    // distance.
    let tol = context_xsect_tol_3d(k, None, SmallTopoType::NormalSize);
    let mut gaps = GapFunction::crv_srf(tol, curve, &interval, surface, 20);
    gaps.build_sample_set(k, false)?;
    gaps.samples()
        .iter()
        .map(|s| {
            Ok(EdgeFaceGapSample {
                edge_parameter: s.this_param().x,
                edge_point: s.this_pos(),
                face_uv: s.other_param(),
                face_point: s.other_pos(),
                distance: point_distance(s.this_pos(), s.other_pos())?,
                surface_bound: surface_bound(k, surface, s.this_pos(), s.other_param())?,
            })
        })
        .collect()
}

/// The largest distance from `p` to the enclosure of S(`uv`), rounded up; `None` where
/// S(`uv`) is not enclosed ([`crate::query::normal::surface_point`]'s `Unsupported` and
/// `Degenerate`: that sample has no bound, the query goes on).
fn surface_bound(k: &Kernel, surface: &Surface, p: Point3, uv: Point2) -> Result<Option<f64>> {
    use crate::blend::fillet_section::interval::{cmax, round_up, Ivl};
    let s = match crate::query::normal::surface_point(k, surface, uv) {
        Ok(s) => s,
        Err(SolidError::Unsupported { .. } | SolidError::Degenerate { .. }) => return Ok(None),
        Err(e) => return Err(e),
    };
    let far = |x: f64, c: Ivl| {
        let m = cmax(
            (Ivl::from(x) - Ivl::from(c.lo)).mag(),
            (Ivl::from(x) - Ivl::from(c.hi)).mag(),
        );
        let m = Ivl::from(round_up(m));
        m * m
    };
    let b = round_up((far(p.x, s[0]) + far(p.y, s[1]) + far(p.z, s[2])).sqrt().hi);
    Ok(b.is_finite().then_some(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::extent2d::Extent2d;
    use crate::nurbs::b_spline_curve::BSplineCurve;
    use crate::nurbs::b_spline_surface::BSplineSurface;
    use crate::nurbs::crv_basic::Curve as NlCurve;
    use crate::nurbs::math::CPoint;
    use crate::nurbs::srf_basic::Surface as NlSurface;

    /// A B-rep without edges or faces is an `SM_ERR` that says so.
    #[test]
    fn an_empty_brep_lists_are_typed_failures() {
        use crate::error::query::QueryFailure;
        let b = Brep::new();
        for (e, what) in [
            (get_edges(&b).unwrap_err(), "the B-rep has no edges"),
            (get_faces(&b).unwrap_err(), "the B-rep has no faces"),
        ] {
            assert_eq!(e.upstream_status(), Some(crate::error::SM_ERR), "{e:?}");
            assert_eq!(e.operation(), OP);
            assert_eq!(e.query_failure(), Some(&QueryFailure::Failed(what)));
        }
    }

    /// The true distance is in `[value - bound, value + bound]` and the value is
    /// `Vec3::distance`'s, bit for bit.
    fn encloses(d: Distance, a: Point3, b: Point3, exact: f64) {
        assert_eq!(d.value.to_bits(), a.distance(b).to_bits());
        assert!(d.bound >= 0.0 && d.bound.is_finite(), "{d:?}");
        assert!((d.value - exact).abs() <= d.bound, "{d:?} vs {exact}");
    }

    #[test]
    fn point_distance_closed_forms_and_their_bounds() {
        // 3-4-12-13: every step exact, the bound only the interval's widening
        let (a, b) = (Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 6.0, 15.0));
        let d = point_distance(a, b).unwrap();
        encloses(d, a, b, 13.0);
        // a few ulps of 13 (the interval's outward widening at each of its 9 steps)
        assert!(d.bound <= 16.0 * f64::EPSILON * 13.0, "{d:?}");
        assert_eq!(point_distance(b, a).unwrap(), d);
        // sqrt 2 is not a double: the bound covers its rounding (about 1e-16)
        let (a, b) = (Point3::ZERO, Point3::new(1.0, 1.0, 0.0));
        let d = point_distance(a, b).unwrap();
        let sqrt2_error = 9.667e-17; // 1.4142135623730950488... - fl(sqrt 2)
        assert!(
            d.bound >= sqrt2_error && d.bound <= 16.0 * f64::EPSILON * d.value,
            "{d:?}"
        );
        // equal points: zero, and a bound at the underflow floor
        let d = point_distance(a, a).unwrap();
        assert_eq!(d.value, 0.0);
        assert!(d.bound <= 1e-149, "{d:?}");
    }

    #[test]
    fn point_distance_is_proved_where_the_squares_underflow() {
        // 3e-170, 4e-170: the squares underflow to zero, so the value is 0 while the
        // exact distance is 5e-170; the bound still covers it
        let (a, b) = (Point3::ZERO, Point3::new(3e-170, 4e-170, 0.0));
        let d = point_distance(a, b).unwrap();
        assert_eq!(d.value, 0.0);
        encloses(d, a, b, 5e-170);
    }

    #[test]
    fn point_distance_refuses_non_finite_and_overflowing_input() {
        let far = Point3::new(1e200, 0.0, 0.0);
        assert!(point_distance(Point3::ZERO, far).is_err());
        let nan = Point3::new(f64::NAN, 0.0, 0.0);
        assert!(point_distance(Point3::ZERO, nan).is_err());
        let inf = Point3::new(0.0, f64::INFINITY, 0.0);
        assert!(point_distance(inf, Point3::ZERO).is_err());
    }

    #[test]
    fn vertex_distance_is_the_point_distance_to_the_vertex() {
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 2.0, 3.0)
            .unwrap();
        let p = Point3::new(4.0, 6.0, 15.0);
        for (v, vertex) in b.vertices().iter() {
            let d = vertex_distance(&b, v, p).unwrap();
            assert_eq!(d, point_distance(vertex.point, p).unwrap());
        }
        let far = Point3::new(1.0, 2.0, 3.0);
        let corner = b
            .vertices()
            .ids()
            .find(|&v| b.vertex(v).unwrap().point == far)
            .unwrap();
        encloses(vertex_distance(&b, corner, p).unwrap(), far, p, 13.0);
    }

    #[test]
    fn edge_face_gap_samples_of_a_box_edge_on_its_own_and_a_far_face() {
        // A box edge on its own face's plane: every sample's gap is 0. On the opposite
        // face's plane (z = 3 against z = 0): every sample's gap is 3.
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 2.0, 3.0)
            .unwrap();
        let faces = get_faces(&b).unwrap();
        let plane_z = |f: FaceId, z: f64| {
            let bx = face_bounding_box(&k, &b, f, true).unwrap();
            (bx.min().z - z).abs() < 1e-12 && (bx.max().z - z).abs() < 1e-12
        };
        let top = *faces.iter().find(|&&f| plane_z(f, 3.0)).unwrap();
        let bottom = *faces.iter().find(|&&f| plane_z(f, 0.0)).unwrap();
        let bottom_edge = get_edges(&b)
            .unwrap()
            .into_iter()
            .find(|&e| {
                let x = edge_bounding_box(&k, &b, e, true).unwrap();
                x.max().z.abs() < 1e-12
            })
            .unwrap();
        let own = edge_face_gap_samples(&k, &b, bottom_edge, bottom).unwrap();
        assert!(!own.is_empty());
        for s in &own {
            assert!(s.distance.value <= 1e-12, "{s:?}");
            encloses(
                s.distance,
                s.edge_point,
                s.face_point,
                s.edge_point.distance(s.face_point),
            );
        }
        let far = edge_face_gap_samples(&k, &b, bottom_edge, top).unwrap();
        assert_eq!(far.len(), own.len());
        for s in &far {
            assert!((s.distance.value - 3.0).abs() <= 1e-12, "{s:?}");
            assert!(s.distance.bound <= 1e-14, "{s:?}");
            // the plane z = 3 is 3 from a point at z = 0: the proved bound holds it
            let sb = s.surface_bound.unwrap();
            assert!((3.0..=3.0 + 1e-12).contains(&sb), "{s:?}");
        }
        for s in &own {
            let sb = s.surface_bound.unwrap();
            assert!(sb >= s.edge_point.z.abs() && sb <= 1e-12, "{s:?}");
        }
    }

    #[test]
    fn a_gap_sample_s_surface_bound_covers_the_exact_surface() {
        // a sphere's seam edge against its own face: the exact distance from each sample's
        // edge point to the sphere, | |p - c| - r |, is within the proved bound, which a
        // rounded face point alone does not give (it is off the sphere by its rounding)
        let mut k = Kernel::default();
        let c = Point3::new(0.3, -0.7, 0.1);
        let r = 1.0 / 3.0;
        let b = crate::primitive::api_primitives::create_sphere(&mut k, c, r).unwrap();
        let f = get_faces(&b).unwrap()[0];
        let mut n = 0;
        for e in get_edges(&b).unwrap() {
            for s in edge_face_gap_samples(&k, &b, e, f).unwrap() {
                let sb = s.surface_bound.unwrap();
                let exact = (point_distance(s.edge_point, c).unwrap().value - r).abs();
                assert!(sb + 1e-15 >= exact, "{s:?}");
                assert!(sb <= 1e-12, "{s:?}");
                n += 1;
            }
        }
        assert!(n > 0);
        // an offset surface is not enclosed: its samples are samples only
        let sphere = b.face(f).unwrap().surface.clone().unwrap();
        let off = Surface::from(
            crate::surface::offset_surface::OffsetSurface::new(&k, 0.1, sphere, true, 0).unwrap(),
        );
        assert_eq!(
            surface_bound(&k, &off, Point3::ZERO, Point2::new(10.0, 5.0)).unwrap(),
            None
        );
    }

    #[test]
    fn a_sample_where_the_weight_is_not_proved_positive_has_no_bound() {
        // a bilinear patch whose weight is 0 at its corner (0, 0): S is not enclosed there
        // (surface_point's Degenerate), so that sample has no bound, and the query goes on;
        // inside the patch the weight is positive and the bound is given
        let k = Kernel::default();
        let knots = vec![0., 0., 1., 1.];
        let s = Surface::from(BSplineSurface::from_nurb(
            NlSurface::new(
                vec![
                    vec![CPoint::new(0., 0., 0., 0.), CPoint::new(0., 1., 0., 1.)],
                    vec![CPoint::new(1., 0., 0., 1.), CPoint::new(1., 1., 0., 1.)],
                ],
                1,
                1,
                knots.clone(),
                knots,
            )
            .unwrap(),
            false,
        ));
        assert!(matches!(
            crate::query::normal::surface_point(&k, &s, Point2::new(0.0, 0.0)),
            Err(SolidError::Degenerate { .. })
        ));
        let p = Point3::new(0.5, 0.5, 1.0);
        assert_eq!(
            surface_bound(&k, &s, p, Point2::new(0.0, 0.0)).unwrap(),
            None
        );
        assert!(surface_bound(&k, &s, p, Point2::new(0.5, 0.5))
            .unwrap()
            .is_some());
    }

    #[test]
    fn edgeuse_face_frame_normals_of_a_box_edge() {
        // every edge use of a box: its face's normal there is that face's axis, the two
        // uses of the face give opposite senses, each proved to rounding; a cylinder's
        // side at an edge of its cap: radial
        let mut k = Kernel::default();
        let mut b =
            crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 2.0, 3.0)
                .unwrap();
        let uses: Vec<EdgeuseId> = b.edgeuses().ids().collect();
        let mut checked = 0;
        for u in uses {
            let Some(f) = eu::face(&b, u).unwrap() else {
                continue;
            };
            let e = b.edgeuse(u).unwrap().edge.unwrap();
            let ivl = b.edge(e).unwrap().interval;
            let t = 0.5 * (ivl.min() + ivl.max());
            let (x, y) = face::faceuses(&b, f).unwrap();
            let (x, y) = (x.unwrap(), y.unwrap());
            let a = edgeuse_face_frame(&k, &mut b, u, t, x).unwrap();
            let c = edgeuse_face_frame(&k, &mut b, u, t, y).unwrap();
            assert_eq!(a.normal.value, -c.normal.value);
            let n = a.normal.value;
            let axis = [n.x.abs(), n.y.abs(), n.z.abs()];
            assert_eq!(axis.iter().filter(|&&v| v == 1.0).count(), 1, "{n:?}");
            // (the net's enclosure widened at each of its few dozen operations)
            assert!(a.normal.bound <= 1e-13, "{:?}", a.normal);
            checked += 1;
        }
        // 12 edges, each in 2 faces, each face with a loop use on each of its 2 face uses
        assert_eq!(checked, 48);
        // another face's faceuse is refused
        let u = b.edgeuses().ids().next().unwrap();
        let f = eu::face(&b, u).unwrap().unwrap();
        let other = get_faces(&b)
            .unwrap()
            .into_iter()
            .find(|&g| g != f)
            .unwrap();
        let (x, _) = face::faceuses(&b, other).unwrap();
        assert!(edgeuse_face_frame(&k, &mut b, u, 0.5, x.unwrap()).is_err());
        // a faceuse whose orientation is unknown has no proved sense: refused, not taken as
        // reversed
        let (x, _) = face::faceuses(&b, f).unwrap();
        let x = x.unwrap();
        b.faceuse_mut(x).unwrap().orientation = OrientType::Unknown;
        assert!(matches!(
            edgeuse_face_frame(&k, &mut b, u, 0.5, x),
            Err(SolidError::Unsupported { .. })
        ));
    }

    #[test]
    fn closed_touching_solids_of_a_box_a_sheet_and_two_boxes_on_an_edge() {
        use crate::geom::a_object::BooleanOperation;
        use crate::primitive::api_primitives::{create_box, create_plane};
        let mut k = Kernel {
            fixes: crate::kernel::UpstreamFixes::all(),
            ..Kernel::default()
        };
        let a = create_box(&mut k, Point3::ZERO, 1.0, 1.0, 1.0).unwrap();
        assert!(brep_is_closed_touching_solids(&a).unwrap());
        let sheet = create_plane(&mut k, Point3::ZERO, 1.0, 1.0).unwrap();
        assert!(!brep_is_closed_touching_solids(&sheet).unwrap());
        // two unit boxes meeting along the edge x = y = 1: closed, not a manifold solid
        let b = create_box(&mut k, Point3::new(1.0, 1.0, 0.0), 1.0, 1.0, 1.0).unwrap();
        let u = crate::boolean::api_brep::boolean(&mut k, a, b, BooleanOperation::Union)
            .unwrap()
            .result;
        assert!(!brep_is_manifold_solid(&u).unwrap());
        assert!(brep_is_closed_touching_solids(&u).unwrap());
    }

    #[test]
    fn material_census_of_a_box_and_an_empty_brep() {
        // the repro of issue 1436: one material region, the infinite region is no void
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_box(
            &mut k,
            Vec3::new(1.0, -2.0, 3.0),
            10.0,
            20.0,
            30.0,
        )
        .unwrap();
        assert_eq!(brep_material_census(&b).unwrap(), (1, 0));
        assert_eq!(brep_material_census(&Brep::new()).unwrap(), (0, 0));
    }

    #[derive(Clone, Copy, PartialEq)]
    struct Object(ObjectKind);
    impl SolutionObject for Object {
        fn kind(&self) -> ObjectKind {
            self.0
        }
    }

    struct Geometry {
        curve: Curve,
        surface: Surface,
    }
    impl Geometry {
        fn new() -> Self {
            let knots = vec![0., 0., 1., 1.];
            Self {
                curve: Curve::from(BSplineCurve::from_nurb(
                    NlCurve::new(
                        vec![CPoint::new(0., 0., 0., 1.), CPoint::new(10., 0., 0., 1.)],
                        1,
                        knots.clone(),
                    )
                    .unwrap(),
                    3,
                    false,
                )),
                surface: Surface::from(BSplineSurface::from_nurb(
                    NlSurface::new(
                        vec![
                            vec![CPoint::new(0., 0., 0., 1.), CPoint::new(0., 1., 0., 1.)],
                            vec![CPoint::new(1., 0., 0., 1.), CPoint::new(1., 1., 0., 1.)],
                        ],
                        1,
                        1,
                        knots.clone(),
                        knots,
                    )
                    .unwrap(),
                    false,
                )),
            }
        }
    }
    impl TopologyObjects<Object> for Geometry {
        fn curve(&self, _: Object) -> Result<&Curve> {
            Ok(&self.curve)
        }
        fn analytic_curve_handle(&self, _: Object) -> Option<Object> {
            None
        }
        fn vertex_point(&self, _: Object) -> Result<Point3> {
            Ok(Point3::ZERO)
        }
        fn vertex_tolerance(&self, _: Object) -> Result<f64> {
            Ok(1e-5)
        }
        fn curve_edge(&self, _: Object) -> Result<Option<Object>> {
            Ok(None)
        }
        fn edge_curve(&self, _: Object) -> Result<Option<Object>> {
            Ok(Some(Object(ObjectKind::Curve)))
        }
        fn edge_interval(&self, _: Object) -> Result<Extent1d> {
            Ok(self.curve.natural_interval())
        }
        fn edge_tolerance(&self, _: Object) -> Result<f64> {
            Ok(1e-5)
        }
        fn surface_face(&self, _: Object) -> Result<Option<Object>> {
            Ok(None)
        }
        fn face_surface(&self, _: Object) -> Result<Object> {
            Ok(Object(ObjectKind::Surface))
        }
        fn surface(&self, _: Object) -> Result<&Surface> {
            Ok(&self.surface)
        }
        fn face_uv_domain(&self, _: Object) -> Result<Extent2d> {
            Ok(self.surface.natural_uv_domain_if_built())
        }
        fn face_tolerance(&self, _: Object) -> Result<f64> {
            Ok(1e-5)
        }
        fn surface_owner(&self, _: Object) -> Result<Option<Object>> {
            Ok(None)
        }
        fn shape_member(&self, _: Object) -> ShapeMember {
            ShapeMember::Other
        }
    }

    #[test]
    fn solution_point_preserves_upstream_parameter_advances() {
        let k = &Kernel::default();
        let objects = Geometry::new();
        for (preceding, expected_x) in [
            (ObjectKind::Surface, 2.5),
            (ObjectKind::Curve, 5.),
            (ObjectKind::Edge, 5.),
            (ObjectKind::Face, 7.5),
        ] {
            let mut solution = Solution::new();
            solution.num_objects = 2;
            solution.objects[0] = Some(Object(preceding));
            solution.objects[1] = Some(Object(ObjectKind::Curve));
            for (i, t) in [0.25, 0.5, 0.75].into_iter().enumerate() {
                *solution.start.param_mut(i).unwrap() = t;
            }
            assert_eq!(
                solution_point(k, &objects, &solution, 1).unwrap(),
                Point3::new(expected_x, 0., 0.),
                "preceding {preceding:?}"
            );
        }
    }

    #[test]
    fn solution_point_ignores_ordinary_evaluation_failures() {
        let k = &Kernel::default();
        let objects = Geometry {
            curve: Curve::from(BSplineCurve::new(3)),
            surface: Surface::from(BSplineSurface::new()),
        };
        assert!(objects.curve.evaluate_point(k, 2.).is_err());
        assert!(objects
            .surface
            .evaluate_point(k, Point2::new(2., 2.))
            .is_err());
        for kind in [
            ObjectKind::Curve,
            ObjectKind::Edge,
            ObjectKind::Surface,
            ObjectKind::Face,
        ] {
            let mut solution = Solution::new();
            solution.num_objects = 1;
            solution.objects[0] = Some(Object(kind));
            *solution.start.param_mut(0).unwrap() = 2.;
            *solution.start.param_mut(1).unwrap() = 2.;
            assert_eq!(
                solution_point(k, &objects, &solution, 0).unwrap(),
                Point3::UNDEF
            );
        }
    }

    #[test]
    fn target_classifier_resolves_notify_named_primitive_box() {
        let mut k = Kernel::default();
        let box_brep =
            crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 1.0, 1.0)
                .unwrap();
        let mut target = Target::new(&box_brep);
        assert_eq!(
            target
                .classify(&k, Point3::new(0.5, 0.5, 0.5), 1e-5, false)
                .unwrap(),
            (PointClassType::Region, true)
        );
        assert_eq!(
            target
                .classify(&k, Point3::new(2.0, 2.0, 2.0), 1e-5, false)
                .unwrap(),
            (PointClassType::Region, false)
        );
    }
}
