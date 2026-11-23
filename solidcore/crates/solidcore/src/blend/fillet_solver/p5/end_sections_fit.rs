//! `SmFilletSolver::EndSectionsFitTheirFaces`: whether a fillet's end section at each
//! Nx1 end lies on its end face.
//!
//! Not upstream (SMLib 1.0.1 has none): the SolidCraft fork's tangency-labs/smlib
//! `a307480` (WR-631), used
//! in the executive's cliff-overflow choice: an end section that runs off its end face
//! under material overflows it, and the blend is built as a cliff overflow is. A
//! material-removing blend crossing a sharp convex boundary of a planar end face is cut
//! the same way: the
//! end face's plane bounds the removal region, and the body clips that region at the
//! face beyond. A tangent continuation keeps the corner's global merge.
//!
//! Deviations: the fork's `sm_SrfSrfIntersection` takes two `SmFilletVertex` objects
//! holding the crossings; [`srf_srf_intersection`] takes the points. The section, a curve
//! in no body, is classified through [`RailClassificationContext::classify_free_curve`]
//! (`SmFace::CurveOnClassify`). Not the fork's (#15767): a rail leaving its face across a
//! smooth edge at a side edge's far end rolls on over it and judges no end section
//! (`leaves_across_tangent_edge_at`), unless the face beyond's offset by the ball folds at
//! the crossing (`ball_folds_beyond`), where the ball cannot roll onto it. There is no
//! recursion.

use super::{RailClassificationContext, LENT_CLASSIFICATION, OP};
use crate::blend::fillet_classification::FilletClassObject;
use crate::blend::fillet_geom::p1::{FilletBrep, FilletGeomType};
use crate::blend::fillet_geom::p2::{srf_srf_intersection, SrfSrfOptions};
use crate::blend::fillet_solver::p1::{
    test_manifold_tangent_rollover, FilletSolver, FilletSolverOps,
};
use crate::curve::curve::Curve;
use crate::error::blend::{BlendFailure, FilletSetUp, SolverFailure};
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::SolverOperation;
use crate::geom::solution_array::{ObjectKind, SolutionObject, SolutionType};
use crate::intersect::curve_class::p1::{CurveClassification, CurveInterval};
use crate::intersect::curve_class::p2::ResetOptions;
use crate::intersect::point_class::{PointClassType, PointClassification};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, BIG_DOUBLE};
use crate::nurbs::knots::Side;
use crate::query::face::p1::{point_3d_classify, Point3dClassifyOptions};
use crate::surface::plane::p1::Plane;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::Surface;
use crate::topology::{edge, edgeuse::p1 as eu, vertex, Brep, EdgeId, FaceId, VertexId};

use super::rails_fit::{edge_bounds_material, is_concave_edge};
use crate::blend::band_room::plane_normal;

/// `smos_Min(a, b)`: `a < b ? a : b`.
fn smos_min(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

/// `smos_Max(a, b)`: `a > b ? a : b`.
fn smos_max(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// The operands of a rail's intersection with the round's section plane at an end of its
/// edge, named as the solver's solutions name them.
#[derive(Debug, Clone, Copy, PartialEq)]
enum SectionOperand {
    /// The rail's curve, with its natural interval.
    Rail(Extent1d),
    /// The section plane, with its natural domain.
    Plane(Extent2d),
}

impl SolutionObject for SectionOperand {
    fn kind(&self) -> ObjectKind {
        match self {
            Self::Rail(_) => ObjectKind::Curve,
            Self::Plane(_) => ObjectKind::Surface,
        }
    }

    fn natural_interval(&self) -> Option<Extent1d> {
        match self {
            Self::Rail(ivl) => Some(*ivl),
            Self::Plane(_) => None,
        }
    }

    fn natural_uv_domain(&self) -> Option<Extent2d> {
        match self {
            Self::Plane(domain) => Some(*domain),
            Self::Rail(_) => None,
        }
    }
}

/// Where rail `curve` meets the end face's plane, the plane through the edge's end `v_point`
/// (a vertex of the end face) with the end face's unit `normal`, taken unbounded: the
/// crossing nearest `v_point`, where the end section starts on that rail (the end is cut by
/// that plane). `None` where the rail does not reach it. Neither the rail's own end nor the
/// plane square to the edge is that point: a rail runs on past the edge's ends (the trace
/// reaches past them), and a curved rail turns away from the side edge's line as it does
/// (#17710); an end face oblique to the edge meets the rail away from the section square to
/// the edge.
fn rail_at_end_plane(
    k: &Kernel,
    curve: &Curve,
    v_point: Point3,
    normal: Vec3,
    tol: f64,
) -> Result<Option<Point3>> {
    let normal = normal.unitize()?;
    // any unit vector square to the normal spans the plane with the third
    let x = if normal.x.abs() < 0.9 {
        Vec3::new(1.0, 0.0, 0.0).cross(normal)
    } else {
        Vec3::new(0.0, 1.0, 0.0).cross(normal)
    }
    .unitize()?;
    let y = normal.cross(x);
    let plane = Surface::from(Plane::create_canonical(&Axis2Placement::new(
        v_point, x, y,
    ))?);
    let ivl = curve.natural_interval();
    let domain = plane.natural_uv_domain_with_nurb(k)?;
    let solutions = plane.global_curve_intersect(
        k,
        [SectionOperand::Rail(ivl), SectionOperand::Plane(domain)],
        &domain,
        curve,
        &ivl,
        tol,
    )?;
    let mut nearest: Option<(f64, Point3)> = None;
    for sol in solutions.as_slice() {
        let mut ts = vec![sol.start.param(0)?];
        if sol.solution_type == SolutionType::RangeOfValues {
            ts.push(sol.end.param(0)?);
        }
        for t in ts {
            let p = curve.evaluate_point(k, t)?;
            let d = p.distance(v_point);
            if nearest.is_none_or(|(n, _)| d < n) {
                nearest = Some((d, p));
            }
        }
    }
    Ok(nearest.map(|(_, p)| p))
}

/// Whether `crossing`, a rail's crossing of its end face's plane, lies past the far end `far`
/// of the side edge from `v_point`: its projection on the side edge's line runs beyond `far`
/// (by more than `tol`).
fn crossing_past_side_edge_end(crossing: Point3, v_point: Point3, far: Point3, tol: f64) -> bool {
    let axis = far - v_point;
    let len = axis.length();
    if len <= tol {
        return false;
    }
    (crossing - v_point).dot(axis) / len > len + tol
}

/// The side edge `side` from `v` with the edges it runs on into between the same two faces,
/// each with its end away from `v`, in order: at each end, the run goes on along the one other
/// edge of those two faces there (a side split by a seam, such as a tube's joint on an end face
/// split by the tube's cylinder seam, #16654), and stops where there is none or more than one,
/// or back at `v` (a closed run). An unsplit side is itself alone. Each end of the run is still
/// judged for what stands on the end face there. The walk ends: an edge joins the run once.
fn side_run(target: &Brep, side: EdgeId, v: VertexId) -> Result<Vec<(EdgeId, VertexId)>> {
    let faces = edge::faces(target, side, None)?;
    let same_faces = |other: &[FaceId]| {
        other.len() == faces.len()
            && other.iter().all(|f| faces.contains(f))
            && faces.iter().all(|f| other.contains(f))
    };
    let mut run = Vec::new();
    let (mut e, mut from) = (side, v);
    loop {
        let (a, b) = edge::start_end_vertices(target, e)?;
        let Some(far) = (if a == Some(from) { b } else { a }) else {
            break;
        };
        run.push((e, far));
        if far == v {
            break;
        }
        let mut next = None;
        let mut ambiguous = false;
        for n in vertex::edges(target, far, None)? {
            if n == e || run.iter().any(|(r, _)| *r == n) {
                continue;
            }
            if same_faces(&edge::faces(target, n, None)?) {
                ambiguous |= next.is_some();
                next = Some(n);
            }
        }
        match next.filter(|_| !ambiguous) {
            Some(n) => (e, from) = (n, far),
            None => break,
        }
    }
    Ok(run)
}

/// Whether crossing an end face's boundary needs the overflow's cut: under material at
/// a concave edge, or into the open at a sharp convex edge of a planar end face for a
/// material-removing blend. The overflow's open-end cut requires that plane.
/// `convex_off_face` means the material-removing section is known to leave the face.
/// Tangent continuations belong to the corner's global merge, not this cut.
fn boundary_requires_cut(
    k: &Kernel,
    target: &mut Brep,
    e: EdgeId,
    end_face: FaceId,
    convex_off_face: bool,
    tangency_tol: f64,
) -> Result<bool> {
    Ok(is_concave_edge(k, target, e)?
        || (convex_off_face
            && edge_bounds_material(target, e)?
            && plane_normal(k, target, end_face)?.is_some()
            && !test_manifold_tangent_rollover(k, target, e, tangency_tol)?.0))
}

/// Whether face `g` has a boundary requiring a cut, other than one of the side's `run`,
/// at vertex `at`.
fn boundary_requires_cut_at(
    k: &Kernel,
    target: &mut Brep,
    run: &[(EdgeId, VertexId)],
    at: VertexId,
    g: FaceId,
    convex_off_face: bool,
    tangency_tol: f64,
) -> Result<bool> {
    for e in vertex::edges(target, at, None)? {
        if run.iter().any(|(r, _)| *r == e) || !edge::faces(target, e, None)?.contains(&g) {
            continue;
        }
        if boundary_requires_cut(k, target, e, g, convex_off_face, tangency_tol)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether a rail, classified on its face as `intervals`, leaves that face (a run off it,
/// `PointClassType::Unknown`) across an edge that ends at `at` and is tangent to the face
/// beyond within `tangency_tol` (`sm_TestManifoldTangentRollover`): the rail rolls over that
/// edge there, as `RailsFitTheirFaces` judges it (#15767).
///
/// Only where the ball can roll onto the face beyond: `ball` is the rail's face's surface
/// and the solver's offset distance d along its natural normal N, so the ball's centre over a
/// point P of that face is `P + d N`. Across the tangent edge, at the rail's crossing X, the
/// face beyond has the same normal line, so its offset by the ball is by `d_b = d (N . N_b)`
/// along its natural normal `N_b`; that offset moves by `(1 - d_b k_i)` times the base's motion
/// along each principal direction, `k_i` the principal curvature positive towards `N_b`
/// (`EvaluateGeometric`'s convention). Where a factor is not positive at X the offset folds
/// back or collapses there (the ball inside a convex cylinder narrower than itself:
/// `1 - 3.5 / 3 < 0`): no ball of the round touches the face beyond next to the edge, the
/// rail does not roll over it, and the end section is judged as on main (the app-built
/// narrowing wall at radius 3.5, #20757's review). The roll-over is taken only where every
/// factor is proved positive: a crossing that does not drop onto either surface, cannot be
/// evaluated there, or a solver without the rail's face's offset (`ball` `None`) proves
/// nothing, and the end section is judged as on main. So does a failure of the test itself
/// (a topology query, the tangency test or an evaluation): main judged the end section
/// without it, so it is `false`, not a bare error main never reached.
///
/// # Errors
/// Those `SolidError::is_unported_or_cancelled` passes on (cancellation, unported callees,
/// unavailable face trims, a spent certificate budget) only.
fn leaves_across_tangent_edge_at(
    k: &Kernel,
    target: &mut Brep,
    intervals: &[CurveInterval<FilletClassObject>],
    curve: &Curve,
    face: FaceId,
    ball: Option<(&Surface, f64)>,
    at: VertexId,
    tangency_tol: f64,
) -> Result<bool> {
    match tangent_leave_proven(k, target, intervals, curve, face, ball, at, tangency_tol) {
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(_) => Ok(false),
        ok => ok,
    }
}

/// The ball over the rail's face `face` for [`leaves_across_tangent_edge_at`]: the base
/// surface of `solver`'s offset of that face and its offset distance. `None` where the
/// solver has no rail on `face`, no offset surface for it, or a lookup fails: the roll-over
/// is then unproven and the end section is judged as on main.
///
/// # Errors
/// Those `SolidError::is_unported_or_cancelled` passes on (cancellation, unported callees,
/// unavailable face trims, a spent certificate budget) only.
fn ball_over_face(
    target: &Brep,
    solver: &FilletSolver,
    face: FaceId,
) -> Result<Option<(Surface, f64)>> {
    let lookup = || -> Result<Option<(Surface, f64)>> {
        for j in 0..2 {
            let Some(seu) = solver.edgeuse(j)? else {
                continue;
            };
            if eu::face(target, seu)? != Some(face) {
                continue;
            }
            if let Some(slot) = solver.surface(j)? {
                return Ok(Some((
                    slot.surface.base_surface().clone(),
                    slot.surface.offset_distance(),
                )));
            }
        }
        Ok(None)
    };
    match lookup() {
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(_) => Ok(None),
        ok => ok,
    }
}

/// The test of [`leaves_across_tangent_edge_at`], `false` where it proves nothing.
///
/// # Errors
/// Those of the topology queries, of the tangency test and of the surface evaluations.
fn tangent_leave_proven(
    k: &Kernel,
    target: &mut Brep,
    intervals: &[CurveInterval<FilletClassObject>],
    curve: &Curve,
    face: FaceId,
    ball: Option<(&Surface, f64)>,
    at: VertexId,
    tangency_tol: f64,
) -> Result<bool> {
    for ivl in intervals {
        if ivl.mid.point_class() != PointClassType::Unknown {
            continue;
        }
        for (pc, t) in [
            (&ivl.start, ivl.interval.min()),
            (&ivl.end, ivl.interval.max()),
        ] {
            if pc.point_class() != PointClassType::Edge {
                continue;
            }
            let Some(o) = pc.object() else {
                continue;
            };
            let EntityRef::Edge(e) = o.target_entity()? else {
                continue;
            };
            if !edge::vertices(target, e, None)?.contains(&Some(at)) {
                continue;
            }
            if !test_manifold_tangent_rollover(k, target, e, tangency_tol)?.0 {
                continue;
            }
            let folds = match ball {
                Some(ball) => {
                    ball_folds_beyond(k, target, e, face, ball, curve.evaluate_point(k, t)?)?
                }
                None => None,
            };
            if folds == Some(false) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Whether the ball, offset `ball.1` along the natural normal of the rail's face's surface
/// `ball.0`, cannot touch the face across tangent edge `e` from `face` at the crossing `x`:
/// the face beyond's offset by it folds or collapses there (see
/// [`leaves_across_tangent_edge_at`]). `Some(true)` where it folds, `Some(false)` where every
/// factor is positive (the ball rolls onto the face beyond), `None` where the crossing proves
/// neither: no face or surface beyond, a drop that fails, an evaluation that fails there (a
/// pole or an apex of the face beyond on the tangent edge), or a factor that is NaN.
///
/// # Errors
/// Cancellation and unported callees only; any other failure of the drops and evaluations
/// at the crossing is `None`.
fn ball_folds_beyond(
    k: &Kernel,
    target: &Brep,
    e: EdgeId,
    face: FaceId,
    ball: (&Surface, f64),
    x: Point3,
) -> Result<Option<bool>> {
    match fold_factors_beyond(k, target, e, face, ball, x) {
        Ok(Some(factors)) => Ok(if factors.iter().any(|&f| f <= 0.0) {
            Some(true)
        } else if factors.iter().all(|&f| f > 0.0) {
            Some(false)
        } else {
            None
        }),
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Ok(None) | Err(_) => Ok(None),
    }
}

/// The factors `1 - d_b k_i` of [`ball_folds_beyond`] at the crossing `x`, `None` where the
/// face beyond, its surface or a drop onto either surface is missing.
///
/// # Errors
/// Those of the topology queries, the drops and the evaluations.
fn fold_factors_beyond(
    k: &Kernel,
    target: &Brep,
    e: EdgeId,
    face: FaceId,
    (base, d): (&Surface, f64),
    x: Point3,
) -> Result<Option<[f64; 2]>> {
    let Some(beyond) = edge::faces(target, e, None)?
        .into_iter()
        .find(|f| *f != face)
    else {
        return Ok(None);
    };
    let Some(surface) = crate::topology::face::p2::surface(target, beyond)? else {
        return Ok(None);
    };
    let on_f = base.drop_point::<NoObject>(
        k,
        x,
        &base.natural_uv_domain_with_nurb(k)?,
        &DropPointOptions::default(),
    )?;
    let on_b = surface.drop_point::<NoObject>(
        k,
        x,
        &surface.natural_uv_domain_with_nurb(k)?,
        &DropPointOptions::default(),
    )?;
    if !on_f.success || !on_b.success {
        return Ok(None);
    }
    let n_f = base.evaluate_normal(k, on_f.result_uv, Side::Left, Side::Left)?;
    let n_b = surface.evaluate_normal(k, on_b.result_uv, Side::Left, Side::Left)?;
    let g = surface.evaluate_geometric(k, on_b.result_uv, Side::Left, Side::Left)?;
    let d_b = if n_f.dot(n_b) < 0.0 { -d } else { d };
    Ok(Some([
        1.0 - d_b * g.principle_curvature1,
        1.0 - d_b * g.principle_curvature2,
    ]))
}

impl FilletSolver {
    /// The fork's `SmFilletSolver::EndSectionsFitTheirFaces` (tangency-labs/smlib
    /// `a307480`): whether the fillet's end section at each Nx1 end lies on the end
    /// face (`false` when it runs off it).
    ///
    /// At a vertex of three edges where only this edge is filleted (an Nx1 corner) the
    /// fillet ends on the third face E, the end face: its end section is where the
    /// fillet's surface meets E's, from rail 0's crossing of its side edge (the edge of
    /// its face and E at the vertex) to rail 1's. Where E is narrower than the fillet and
    /// something stands on it next to the vertex (a lip on a shelled box's rim, the
    /// section running under the lip's foot: off E across a concave edge of it), the
    /// fillet overflows its end face. A material-removing blend whose section crosses a
    /// sharp convex edge of planar E is cut by the same construction, clipped to the body beyond
    /// E. A tangent continuation keeps the corner's global merge. The section is the
    /// surfaces' exact intersection between the two crossings, classified against E.
    /// Judged only where the corner
    /// takes the end face as it is: both side edges of the filleted edge's convexity,
    /// neither tangent.
    ///
    /// # Errors
    /// Missing rails, faces, edge uses or surfaces (upstream dereferences them); `SM_ERR`
    /// when a crossing does not drop onto the section within ten times the solver's
    /// tolerance; those of the classifications, the concavity and tangency tests, the
    /// intersection and the corner lookup.
    #[allow(clippy::too_many_lines)] // one fork routine
    pub fn end_sections_fit_their_faces_with_ops(
        solver: &mut dyn FilletSolverOps,
        k: &Kernel,
        target: &mut Brep,
        fillets: &mut FilletBrep,
        context: &mut impl RailClassificationContext,
    ) -> Result<bool> {
        let mut fit = true;
        let fil_eu = solver.solver().edgeuse(0)?.ok_or_else(|| {
            SolidError::blend_solver(OP, SolverFailure::SolverEdgeUseMissing { index: 0 })
        })?;
        let fil_edge = eu::edge(target, fil_eu)?.ok_or_else(|| {
            SolidError::blend_at(
                OP,
                BlendFailure::EdgeUseWithoutEdge,
                EntityRef::Edgeuse(fil_eu),
            )
        })?;
        // A sheet's edge bounds no material: no end section can run off its end face under
        // material, and there is no body to subtract an overflow from, so the corner trims
        // the fillet as it does any other (not the fork's, whose `IsConcaveEdge` fails there,
        // and the fillet with it).
        if !edge_bounds_material(target, fil_edge)? {
            return Ok(true);
        }
        let concave = is_concave_edge(k, target, fil_edge)?;
        let approx_tol = solver.solver().this_approx_tol_3d();
        let ang_tol = solver.solver().this_ang_tol_rad();
        let tangency_tol = solver.solver().tangency_tolerance;
        for id in solver.solver().fillet_geoms() {
            if !fit {
                break;
            }
            if fillets.geom(id)?.fillet_geom_type() != FilletGeomType::Default {
                continue;
            }
            let fillet_surface = fillets
                .geom(id)?
                .fillet_surface(None)?
                .ok_or_else(|| {
                    SolidError::blend_solver(
                        OP,
                        SolverFailure::FilletGeomWithoutSurface { fillet_geom: id },
                    )
                })?
                .clone();
            let (start, end) = edge::start_end_vertices(target, fil_edge)?;
            for v in [start, end] {
                let v = v.ok_or_else(|| {
                    SolidError::blend_solver(
                        OP,
                        SolverFailure::EdgeWithoutVertex { edge: fil_edge },
                    )
                })?;
                let corner = context.corner_of_vertex(v)?;
                let vertex_edges = vertex::edges(target, v, None)?;
                // the fork's IsEndOnEndFace() (53c47b8; WR-697): the Nx1 corner pinned
                let is_nx1 = match corner {
                    Some(c) => context.corner_blend_tests(c)?.1,
                    None => false,
                };
                if !is_nx1 || vertex_edges.len() != 3 {
                    continue;
                }
                let v_point = vertex::point(target, v)?;
                // each rail's side edge at the vertex, the end face they share, and where
                // the rail crosses it
                let mut end_face: Option<FaceId> = None;
                let mut cross = [Point3::default(); 2];
                let mut both = true;
                for (ii, cross_at) in cross.iter_mut().enumerate() {
                    if !both {
                        break;
                    }
                    let rail = fillets.geom(id)?.rail(ii).ok_or_else(|| {
                        SolidError::blend_solver(
                            OP,
                            SolverFailure::FilletGeomWithoutRail {
                                fillet_geom: id,
                                side: ii,
                            },
                        )
                    })?;
                    let curve = edge::curve(fillets.brep(), rail)?
                        // a rail edge with no curve is [`FilletSetUp::RailCurve`]
                        .ok_or_else(|| SolidError::blend(OP, BlendFailure::FilletSetUpInvalid(FilletSetUp::RailCurve)))?
                        .clone();
                    let face = fillets.fillet_edge(rail)?.original_face().ok_or_else(|| {
                        SolidError::blend_solver(OP, SolverFailure::RailWithoutFace { rail })
                    })?;
                    let primary = eu::primary_edgeuse(fillets.brep(), rail)?.ok_or_else(|| {
                        SolidError::blend_solver(OP, SolverFailure::RailEdgeUseMissing { rail })
                    })?;
                    let mate = eu::mate(fillets.brep(), primary)?.ok_or_else(|| {
                        SolidError::blend_solver(OP, SolverFailure::RailEdgeUseMissing { rail })
                    })?;
                    let mut side = None;
                    for &e in &vertex_edges {
                        let faces = edge::faces(target, e, None)?;
                        if e != fil_edge && faces.len() == 2 && faces.contains(&face) {
                            side = Some(e);
                            let other = if faces.first() == Some(&face) {
                                faces.get(1)
                            } else {
                                faces.first()
                            }
                            .copied();
                            if end_face.is_some() && end_face != other {
                                side = None;
                            }
                            end_face = other;
                        }
                    }
                    let Some(side) = side.filter(|_| end_face.is_some()) else {
                        both = false;
                        continue;
                    };
                    // An end face the corner takes as it is: a side edge of the filleted
                    // edge's convexity, and not tangent
                    let side_concave = is_concave_edge(k, target, side)?;
                    if side_concave != concave
                        || test_manifold_tangent_rollover(k, target, side, tangency_tol)?.0
                    {
                        both = false;
                        continue;
                    }
                    // the rail's crossing of the side nearest the vertex: of the side edge
                    // or of an edge it runs on into (a side split by a seam is one side)
                    let run = side_run(target, side, v)?;
                    let mut cc = CurveClassification::new(k);
                    cc.reset(
                        k,
                        &ResetOptions {
                            curve: Some(context.rail_curve(rail)),
                            interval: Some(curve.natural_interval()),
                            uv_curve: context.rail_mate_uv_curve(fillets.brep(), mate),
                            zone_tol_3d: approx_tol,
                            ..ResetOptions::default()
                        },
                    )?;
                    let attached = fillets.fillet_edge_mut(rail)?.curve_class.replace(cc);
                    let classified = fillets.with_rail_classification(rail, |pseudo, cc| {
                        context.classify(k, target, pseudo, face, cc)
                    });
                    let intervals = fillets
                        .fillet_edge(rail)?
                        .curve_class()
                        .map(|cc| cc.intervals().to_vec());
                    fillets.fillet_edge_mut(rail)?.curve_class = attached;
                    classified?;
                    let intervals = intervals
                        .ok_or_else(|| SolidError::blend_null_defect(OP, LENT_CLASSIFICATION))?;
                    let mut nearest = BIG_DOUBLE;
                    for ivl in &intervals {
                        for (pc, t) in [
                            (&ivl.start, ivl.interval.min()),
                            (&ivl.end, ivl.interval.max()),
                        ] {
                            let on_side = pc.point_class() == PointClassType::Edge
                                && match pc.object() {
                                    Some(o) => {
                                        matches!(o.target_entity(), Ok(EntityRef::Edge(e)) if run.iter().any(|(r, _)| *r == e))
                                    }
                                    None => false,
                                };
                            if !on_side {
                                continue;
                            }
                            let at = curve.evaluate_point(k, t)?;
                            let dist = at.distance(v_point);
                            if dist < nearest {
                                nearest = dist;
                                *cross_at = at;
                            }
                        }
                    }
                    if nearest == BIG_DOUBLE {
                        // The rail crosses no side edge at the vertex because it runs
                        // past the side edge's far end P
                        // (where it meets the end face's plane, the end section's start,
                        // projects past P on the side edge's line), and the end face has a
                        // boundary requiring a cut at P: the end section lies past that face.
                        // Its plane cuts the removal region, and subtraction clips it to the
                        // body. Tangent continuations keep the corner's global merge.
                        // A rail that crosses no side edge because its face runs on past
                        // the end face (the side edge
                        // leaves the vertex the other way) is short of the vertex there, not
                        // past P (#17710).
                        // P is each end along the side's run: a side split only by a seam
                        // (the joint of a tube, split by the tube's seam) has no end at the
                        // split, and the run's next edge there is the side, not something
                        // standing on the end face (#16654).
                        if let Some(g) = end_face {
                            // an end face that is not planar is not cut by a plane: not
                            // judged, as the fork
                            let crossing = match plane_normal(k, target, g)? {
                                Some(normal) => {
                                    rail_at_end_plane(k, &curve, v_point, normal, approx_tol)?
                                }
                                None => None,
                            };
                            // the ball over the rail's face: its surface and the solver's
                            // offset of it (#15767)
                            let ball_slot = ball_over_face(target, solver.solver(), face)?;
                            for &(_, far) in &run {
                                // #15767: a rail that leaves its face across a smooth edge
                                // ending at P rolls on over that edge onto the face beyond
                                // (`RailsFitTheirFaces`' rollover, `ProcessRollOver`): its
                                // crossing past P is the rail continued on its face's
                                // surface, not the rolled rail, and judges no end section
                                if leaves_across_tangent_edge_at(
                                    k,
                                    target,
                                    &intervals,
                                    &curve,
                                    face,
                                    ball_slot.as_ref().map(|(base, d)| (base, *d)),
                                    far,
                                    tangency_tol,
                                )? {
                                    continue;
                                }
                                let p = vertex::point(target, far)?;
                                if let Some(x) = crossing.filter(|x| {
                                    crossing_past_side_edge_end(*x, v_point, p, approx_tol)
                                }) {
                                    // Past P can run into an L-shaped end face instead of
                                    // leaving it. Only an off-face crossing admits the new
                                    // convex cut; concave boundaries keep their existing rule.
                                    let convex_off_face = !concave && {
                                        let mut pc =
                                            PointClassification::<EntityRef>::new(approx_tol);
                                        point_3d_classify(
                                            k,
                                            target,
                                            g,
                                            x,
                                            approx_tol,
                                            true,
                                            &mut pc,
                                            Point3dClassifyOptions::default(),
                                        )?;
                                        pc.point_class() == PointClassType::Unknown
                                    };
                                    if boundary_requires_cut_at(
                                        k,
                                        target,
                                        &run,
                                        far,
                                        g,
                                        convex_off_face,
                                        tangency_tol,
                                    )? {
                                        fit = false;
                                        break;
                                    }
                                }
                            }
                        }
                        both = false;
                    }
                }
                let Some(end_face) = end_face.filter(|_| both) else {
                    continue;
                };
                let Some(end_surface) = crate::topology::face::p2::surface(target, end_face)?
                else {
                    continue;
                };
                let end_surface = end_surface.clone();
                // the end section, and whether the end face holds it
                let [from, to] = cross;
                let curves = srf_srf_intersection(
                    k,
                    Some(from),
                    Some(to),
                    None,
                    None,
                    &fillet_surface,
                    &end_surface,
                    approx_tol,
                    ang_tol,
                    SrfSrfOptions::default(),
                )?;
                let section = curves.curve_3d.ok_or_else(|| {
                    SolidError::blend_null_defect(
                        OP,
                        "the end section's intersection succeeded without a 3D curve: \
                             `srf_srf_intersection` returns its 3D curve on every success",
                    )
                })?;
                // the section from one crossing to the other (the intersection may run
                // on past them)
                let mut ends = [0.0; 2];
                for (p, at) in cross.iter().zip(ends.iter_mut()) {
                    let dropped = section.drop_point(
                        k,
                        &section.natural_interval(),
                        *p,
                        None,
                        BIG_DOUBLE,
                        None,
                        SolverOperation::Minimize,
                    )?;
                    if !dropped.success {
                        return Err(SolidError::blend_solver(
                            OP,
                            SolverFailure::CrossingDoesNotDropOnEndSection,
                        ));
                    }
                    if dropped.distance > 10.0 * approx_tol {
                        return Err(SolidError::blend_solver(
                            OP,
                            SolverFailure::CrossingOffEndSection {
                                distance: dropped.distance,
                                limit: 10.0 * approx_tol,
                            },
                        ));
                    }
                    *at = dropped.parameter;
                }
                let between =
                    Extent1d::from_min_max(smos_min(ends[0], ends[1]), smos_max(ends[0], ends[1]));
                let intervals = context.classify_free_curve(
                    k,
                    target,
                    fillets.brep(),
                    end_face,
                    &section,
                    between,
                    curves.uv_curve2.as_ref(),
                    approx_tol,
                )?;
                let n = intervals.len();
                for (j, ivl) in intervals.iter().enumerate() {
                    if !fit {
                        break;
                    }
                    if ivl.mid.point_class() != PointClassType::Unknown {
                        continue;
                    }
                    // The section's ends lie on the side edges: a first or last run off the
                    // face no longer than the classification's tolerance is its end on
                    // that edge.
                    if j == 0 || j + 1 == n {
                        let a = section.evaluate_point(k, ivl.interval.min())?;
                        let b = section.evaluate_point(k, ivl.interval.max())?;
                        if a.distance(b) <= approx_tol {
                            continue;
                        }
                    }
                    // The same boundary event as a rail past its side edge's far end:
                    // the end face no longer holds the section, so its plane cuts the
                    // removal region and the body clips it at the boundary beyond.
                    let mut leaves = false;
                    for pc in [&ivl.start, &ivl.end] {
                        if pc.point_class() != PointClassType::Edge {
                            continue;
                        }
                        let Some(o) = pc.object() else {
                            continue;
                        };
                        let EntityRef::Edge(e) = o.target_entity()? else {
                            return Err(SolidError::blend_solver(
                                OP,
                                SolverFailure::ClassificationEdgePointNotOnEdge,
                            ));
                        };
                        if boundary_requires_cut(k, target, e, end_face, !concave, tangency_tol)? {
                            leaves = true;
                        }
                    }
                    if leaves {
                        fit = false;
                    }
                }
            }
        }
        Ok(fit)
    }
}
