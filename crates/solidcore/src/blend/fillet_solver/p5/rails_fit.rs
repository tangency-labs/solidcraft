//! `SmFilletSolver::RailsFitTheirFaces` and `LeavesPastAnEnd`: whether every rail of a
//! solver's fillet stays on the face it rolls on.
//!
//! Not upstream (SMLib 1.0.1 has neither): the fork's check, tangency-labs/smlib
//! `fc43974` (refusal), `967f7ab` (`LeavesPastAnEnd`) and `518566f` (a run of off-face
//! intervals is judged once, WR-302, tangency-labs/solidcore#841 and #1024). Used only
//! with the opt-in `Kernel::fixes.fillet_refuse_rails_leaving_faces`.
//!
//! Deviations: a run of off-face intervals between two stretches on the face is skipped
//! when it both leaves and returns past an end of the filleted edge (#1580); `518566f`
//! tries [`leaves_past_an_end`] only for the first and last runs and refuses such a
//! round. The fork's later cliff-overflow construction (`SmFilletCliffOverflow.cpp`,
//! which builds a convex blend over a cliff and subtracts what it removes) is not
//! ported, so every blend that leaves its face between its ends is refused, as in
//! `fc43974`. With the opt-in `Kernel::fixes.fillet_cliff_overflow` (WR-631, the fork's
//! `8ba4667` at rev `68fb9162`) the check also tells whether a rail rolls over a tangent
//! edge and whether a rail that leaves its face runs on into the body's material
//! ([`RailsFit`], `IsConcaveEdge`, `sm_InsideMaterial`), for the executive's choice
//! between a refusal and a cliff overflow; those judgements run only with that flag. The
//! fork's `IsKnifeEdge` ([`is_knife_edge`], `e418750`, WR-665) is here too, used by
//! `SmFilletCorner::Create` with `fixes.fillet_corner_per_vertex_fan`. The
//! nearest-edge search of `sm_InsideMaterial` keeps the first of equally near edges, in
//! the face's edge order, as the fork's strict `<` does. There is no recursion.

use super::{RailClassificationContext, OP};
use crate::blend::fillet_classification::FilletClassObject;
use crate::blend::fillet_geom::p1::{FilletBrep, FilletGeomType};
use crate::blend::fillet_solver::p1::{
    test_manifold_tangent_rollover, FilletSolver, FilletSolverOps,
};
use crate::curve::curve::Curve;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::global_solver::SolverOperation;
use crate::intersect::curve_class::p1::CurveClassification;
use crate::intersect::curve_class::p2::ResetOptions;
use crate::intersect::point_class::{PointClassType, PointClassification};
use crate::kernel::Kernel;
use crate::math::Point3;
use crate::math::BIG_DOUBLE;
use crate::topology::edgeuse::p2 as eu2;
use crate::topology::face::p4 as face_p4;
use crate::topology::{edge, edgeuse::p1 as eu, Brep, EdgeId, FaceId};

/// What [`FilletSolver::rails_fit_their_faces_judged_with_ops`] finds of a solver's rails
/// (the fork's `RailsFitTheirFaces(rbFit, rbRollsOver, rbIntoMaterial)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RailsFit {
    /// `rbFit`: every rail stays on its face (`false` = a rail leaves its face between
    /// the filleted edge's ends, or is nowhere on it).
    pub fit: bool,
    /// `rbRollsOver`: a rail rolls over a tangent edge onto the next face.
    pub rolls_over: bool,
    /// `rbIntoMaterial`: a rail that leaves its face runs on inside the body (judged only
    /// when asked; `false` otherwise).
    pub into_material: bool,
}

/// `SmFilletSolver::IsKnifeEdge` (the fork's, tangency-labs/smlib `e418750`, WR-665):
/// whether `edge_id` is a knife edge, its two faces tangent along it and folded back on
/// each other, the body's material between them of zero angle (a hole whose wall
/// touches the bottom face along a line). The sector from the edge's primary edge use to
/// its radial at the middle of the edge's interval (`SmEdgeuse::CalcSectorAngle`) is
/// within `tangency_tol_rad` of none or a whole turn (a smooth edge's is a half turn).
/// An edge without primary edge use or radial, or whose sector cannot be computed, is
/// not one.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::Unported`] from the binormal
/// evaluation (any other failure of the sector angle reads as not a knife edge, as the
/// fork's status test does).
pub fn is_knife_edge(
    k: &Kernel,
    target: &mut Brep,
    edge_id: EdgeId,
    tangency_tol_rad: f64,
) -> Result<bool> {
    let Some(primary) = eu::primary_edgeuse(target, edge_id)? else {
        return Ok(false);
    };
    if eu::radial(target, primary)?.is_none() {
        return Ok(false);
    }
    // the sector between the faces, a whole turn or none (a smooth edge's is a half turn)
    let parameter = edge::interval(target, edge_id)?.evaluate(0.5);
    let angle = match crate::topology::edgeuse::p3::calc_sector_angle(k, target, primary, parameter)
    {
        Ok(angle) => angle,
        Err(e @ SolidError::Unported { .. }) => return Err(e),
        Err(_) => return Ok(false),
    };
    // smos_Min
    let other = crate::math::TWO_PI - angle;
    let smaller = if angle < other { angle } else { other };
    Ok(smaller < tangency_tol_rad)
}

/// `SmFilletSolver::IsConcaveEdge` (fork, WR-631): whether the body's material wedge
/// at `edge` spans more than a half turn (a wall's foot), rather than less (a cliff's
/// rim). The first loop edge use of the edge on the material side (its shell's region
/// not void) is asked `IsConvexRadialSector(10)`. A tangent edge is not concave.
///
/// # Errors
///
/// `SM_ERR` for an edge with no loop edge use on the material side; those of the
/// sector test.
pub fn is_concave_edge(k: &Kernel, target: &mut Brep, edge_id: EdgeId) -> Result<bool> {
    for use_ in edge::edgeuses(target, edge_id, None, None)? {
        if !eu::is_loop_edgeuse(target, use_)? {
            continue;
        }
        let Some(shell) = eu::shell(target, use_)? else {
            continue;
        };
        let Some(region) = crate::topology::shell::shell_region(target, shell)? else {
            continue;
        };
        // `IsVoid()` as a C++ boolean: any non-zero flag
        if target.region(region)?.is_void() != 0 {
            continue;
        }
        return eu2::is_convex_radial_sector(k, target, use_, 10, false, 0.0);
    }
    Err(SolidError::sm_err(OP))
}

/// `sm_InsideMaterial` (fork, WR-631): whether a rail that leaves `face` near `point`
/// runs on into the body's material, judged by the face's boundary edge nearest
/// `point` ([`is_concave_edge`]).
///
/// # Errors
///
/// `SM_ERR_NULL_POINTER` when no edge of the face takes the point's drop; those of the
/// drops and of [`is_concave_edge`].
pub fn inside_material(k: &Kernel, target: &mut Brep, face: FaceId, point: Point3) -> Result<bool> {
    let mut nearest = None;
    let mut nearest_dist = BIG_DOUBLE;
    for e in face_p4::edges(target, face, None, false, None)? {
        let Some(curve) = edge::curve(target, e)? else {
            continue;
        };
        let ivl = edge::interval(target, e)?;
        let dropped = curve.drop_point(
            k,
            &ivl,
            point,
            None,
            BIG_DOUBLE,
            None,
            SolverOperation::Minimize,
        )?;
        if dropped.success && dropped.distance < nearest_dist {
            nearest_dist = dropped.distance;
            nearest = Some(e);
        }
    }
    let nearest = nearest.ok_or_else(|| SolidError::null_pointer(OP))?;
    is_concave_edge(k, target, nearest)
}

/// `SmFilletSolver::LeavesPastAnEnd`: whether a rail that leaves the face it rolls on at
/// `leave` (rail parameter `rail_param`) does so past an end of the filleted edge, where
/// the corner at that end trims it; otherwise it leaves the face beside the edge.
///
/// At an end the fillet stops in one of two ways, and its rail leaves the face only after
/// it: the face ends there in its side edge (the edge before or after the filleted edge
/// in the face's loop), and the rail leaves across that side edge or through one of its
/// vertices; or the face goes on past the end, and the rail stays on it to the end's cross
/// section and beyond (the edge point nearest where it leaves is an end).
fn leaves_past_an_end(
    solver: &FilletSolver,
    k: &Kernel,
    target: &Brep,
    face: FaceId,
    rail: &Curve,
    rail_param: f64,
    leave: &PointClassification<FilletClassObject>,
) -> Result<bool> {
    let fil_eu = solver
        .edgeuse(0)?
        .ok_or_else(|| SolidError::null_pointer(OP))?;
    let fil_edge = eu::edge(target, fil_eu)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    let edge_curve = edge::curve(target, fil_edge)?.ok_or_else(|| SolidError::null_pointer(OP))?;

    // Across or through the face's side edge at an end.
    let (leave_edge, leave_vertex) = match (leave.point_class(), leave.object()) {
        (PointClassType::Edge, Some(o)) => match o.target_entity()? {
            EntityRef::Edge(e) => (Some(e), None),
            _ => (None, None),
        },
        (PointClassType::Vertex, Some(o)) => match o.target_entity()? {
            EntityRef::Vertex(v) => (None, Some(v)),
            _ => (None, None),
        },
        _ => (None, None),
    };
    for use_ in edge::edgeuses(target, fil_edge, None, None)? {
        if eu::face(target, use_)? != Some(face) || !eu::is_loop_edgeuse(target, use_)? {
            continue;
        }
        for next in [
            eu::cw_edgeuse(target, use_)?,
            eu::ccw_edgeuse(target, use_)?,
        ] {
            let Some(side) = next.map(|n| eu::edge(target, n)).transpose()?.flatten() else {
                continue;
            };
            if side == fil_edge {
                continue;
            }
            let first = edge::vertex(target, side)?;
            let past = leave_edge == Some(side)
                || leave_vertex.is_some_and(|v| {
                    first == Some(v)
                        || edge::other_vertex(target, side, first).is_ok_and(|o| o == Some(v))
                });
            if past {
                return Ok(true);
            }
        }
    }

    // On or beyond the cross section at an end.
    let ivl = edge::interval(target, fil_edge)?;
    let leave_point = rail.evaluate_point(k, rail_param)?;
    let dropped = edge_curve.drop_point(
        k,
        &ivl,
        leave_point,
        None,
        BIG_DOUBLE,
        None,
        SolverOperation::Minimize,
    )?;
    if !dropped.success {
        return Err(SolidError::sm_err(OP));
    }
    let foot = edge_curve.evaluate_point(k, dropped.parameter)?;
    for end in [ivl.min(), ivl.max()] {
        if foot.distance(edge_curve.evaluate_point(k, end)?) <= solver.approx_tol_3d {
            return Ok(true);
        }
    }
    Ok(false)
}

impl FilletSolver {
    /// `SmFilletSolver::RailsFitTheirFaces` (the fork's refusal, see the module doc):
    /// whether every rail of this solver's fillet, untrimmed, stays on the face it rolls
    /// on. Each rail is classified against its face. It may run off the face past the
    /// filleted edge's ends (`leaves_past_an_end`) and off across a tangent side edge (a
    /// rollover onto the next face). A rail that is nowhere on its face, or that leaves it
    /// between its ends across an edge that is not tangent (a wall, an opening's rim)
    /// means the blend is larger than the faces: `false`. Touching or running along the
    /// face's boundary is on the face.
    ///
    /// The classification is made as `TestRollOver`'s is and is not kept: each rail's
    /// attached classification is as it was.
    ///
    /// # Errors
    /// Missing rails, faces or edge uses (upstream dereferences them); those of the
    /// classification, the drops and the tangency test.
    pub fn rails_fit_their_faces_with_ops(
        solver: &mut dyn FilletSolverOps,
        k: &Kernel,
        target: &mut Brep,
        fillets: &mut FilletBrep,
        context: &mut impl RailClassificationContext,
    ) -> Result<bool> {
        Self::rails_fit_their_faces_judged_with_ops(solver, k, target, fillets, context, false)
            .map(|judged| judged.fit)
    }

    /// The fork's `RailsFitTheirFaces(rbFit, rbRollsOver, rbIntoMaterial)` (WR-631): as
    /// [`FilletSolver::rails_fit_their_faces_with_ops`], and whether a rail rolls over a
    /// tangent edge, and, when `judge_material`, whether a rail that leaves its face
    /// runs on into the body's material: the edge it leaves across is concave
    /// ([`is_concave_edge`]), or for a rail nowhere on its face (or leaving it where no
    /// edge is classified) the face's edge nearest its point nearest the edge's middle
    /// ([`inside_material`]).
    ///
    /// # Errors
    /// As [`FilletSolver::rails_fit_their_faces_with_ops`]; those of the material
    /// judgements, and `SM_ERR` when the filleted edge's middle does not drop onto a rail
    /// nowhere on its face.
    #[allow(clippy::too_many_lines)] // one fork routine
    pub fn rails_fit_their_faces_judged_with_ops(
        solver: &mut dyn FilletSolverOps,
        k: &Kernel,
        target: &mut Brep,
        fillets: &mut FilletBrep,
        context: &mut impl RailClassificationContext,
        judge_material: bool,
    ) -> Result<RailsFit> {
        let mut fit = true;
        let mut rolls_over = false;
        let mut into_material = false;
        for id in solver.solver().fillet_geoms() {
            if fillets.geom(id)?.fillet_geom_type() != FilletGeomType::Default {
                continue;
            }
            for ii in 0..2 {
                let rail = fillets
                    .geom(id)?
                    .rail(ii)
                    .ok_or_else(|| SolidError::null_pointer(OP))?;
                let curve = edge::curve(fillets.brep(), rail)?
                    .ok_or_else(|| SolidError::null_pointer(OP))?
                    .clone();
                let face = fillets
                    .fillet_edge(rail)?
                    .original_face()
                    .ok_or_else(|| SolidError::null_pointer(OP))?;
                let primary = eu::primary_edgeuse(fillets.brep(), rail)?
                    .ok_or_else(|| SolidError::null_pointer(OP))?;
                let mate = eu::mate(fillets.brep(), primary)?
                    .ok_or_else(|| SolidError::null_pointer(OP))?;
                let mut cc = CurveClassification::new(k);
                cc.reset(
                    k,
                    &ResetOptions {
                        curve: Some(context.rail_curve(rail)),
                        interval: Some(curve.natural_interval()),
                        uv_curve: context.rail_mate_uv_curve(fillets.brep(), mate),
                        zone_tol_3d: solver.solver().approx_tol_3d,
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
                let intervals = intervals.ok_or_else(|| SolidError::null_pointer(OP))?;

                let n = intervals.len();
                let off_face = |j: usize| {
                    intervals
                        .get(j)
                        .is_some_and(|i| i.mid.point_class() == PointClassType::Unknown)
                };
                let mut on_face = false;
                for (j, ivl) in intervals.iter().enumerate() {
                    if ivl.mid.point_class() != PointClassType::Unknown {
                        on_face = true;
                        continue;
                    }
                    // A run of intervals off the face (split where the rail passes through
                    // a point of the face's boundary from outside to outside) leaves the
                    // face once: judged at its first interval, from where it leaves to the
                    // rail's end or start.
                    if j > 0 && off_face(j - 1) {
                        continue;
                    }
                    let mut j_last = j;
                    while off_face(j_last + 1) {
                        j_last += 1;
                    }
                    let last = intervals
                        .get(j_last)
                        .ok_or_else(|| SolidError::null_pointer(OP))?;
                    // Where the rail leaves the face: the first interval ends there, the
                    // others start.
                    let (leave, d_leave) = if j == 0 {
                        (&last.end, last.interval.max())
                    } else {
                        (&ivl.start, ivl.interval.min())
                    };
                    let past_end = |point: &PointClassification<FilletClassObject>, d: f64| {
                        leaves_past_an_end(solver.solver(), k, target, face, &curve, d, point)
                    };
                    if j == 0 || j_last + 1 == n {
                        if past_end(leave, d_leave)? {
                            continue;
                        }
                    } else if past_end(leave, d_leave)? && past_end(&last.end, last.interval.max())?
                    {
                        // A run between two stretches on the face that lies wholly past an
                        // end (it leaves and comes back past it) is trimmed by the corner
                        // there too (tangency-labs/solidcore#1580; the fork's `518566f`
                        // tried only the first and last runs).
                        continue;
                    }
                    let side = match (leave.point_class(), leave.object()) {
                        (PointClassType::Edge, Some(o)) => match o.target_entity()? {
                            EntityRef::Edge(e) => Some(e),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(side) = side {
                        let tolerance = solver.solver().tangency_tolerance;
                        if test_manifold_tangent_rollover(k, target, side, tolerance)?.0 {
                            rolls_over = true;
                            continue; // a rollover onto the next face
                        }
                    }
                    fit = false;
                    if judge_material {
                        // the edge it leaves across (or the face's edge nearest where it leaves)
                        let inside = if let Some(side) = side {
                            is_concave_edge(k, target, side)?
                        } else {
                            let leave_point = curve.evaluate_point(k, d_leave)?;
                            inside_material(k, target, face, leave_point)?
                        };
                        if inside {
                            into_material = true;
                        }
                    }
                }
                if !on_face {
                    fit = false;
                    if judge_material {
                        // the rail's point nearest the middle of the filleted edge
                        let fil_eu = solver
                            .solver()
                            .edgeuse(0)?
                            .ok_or_else(|| SolidError::null_pointer(OP))?;
                        let fil_edge = eu::edge(target, fil_eu)?
                            .ok_or_else(|| SolidError::null_pointer(OP))?;
                        let fil_curve = edge::curve(target, fil_edge)?
                            .ok_or_else(|| SolidError::null_pointer(OP))?;
                        let mid = fil_curve
                            .evaluate_point(k, edge::interval(target, fil_edge)?.evaluate(0.5))?;
                        let dropped = curve.drop_point(
                            k,
                            &curve.natural_interval(),
                            mid,
                            None,
                            BIG_DOUBLE,
                            None,
                            SolverOperation::Minimize,
                        )?;
                        if !dropped.success {
                            return Err(SolidError::sm_err(OP));
                        }
                        let on_rail = curve.evaluate_point(k, dropped.parameter)?;
                        if inside_material(k, target, face, on_rail)? {
                            into_material = true;
                        }
                    }
                }
            }
        }
        Ok(RailsFit {
            fit,
            rolls_over,
            into_material,
        })
    }
}
