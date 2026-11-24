//! `SmFilletSolver::RailsFitTheirFaces` and `LeavesPastAnEnd`: whether every rail of a
//! solver's fillet stays on the face it rolls on.
//!
//! Not upstream (SMLib 1.0.1 has neither): the fork's check, tangency-labs/smlib
//! `fc43974` (refusal), `967f7ab` (`LeavesPastAnEnd`) and `518566f` (a run of off-face
//! intervals is judged once, WR-302, tangency-labs/solidcore#841 and #1024).
//!
//! Deviations: a run of off-face intervals between two stretches on the face is skipped
//! when it both leaves and returns on or beyond the cross section at the same end of the
//! filleted edge (#1580; a run past one end and back past the other, round the far side
//! of a closed rail, is judged, #2175); `518566f` tries [`leaves_past_an_end`] only for
//! the first and last runs and refuses such a round. Such a run's leave (or return) is
//! also past an end where it crosses the face's side edge at that end heading out past it
//! (or in from beyond it), as for a first or last run ([`past_end_through_side_edge`]): a
//! rail run on round a closed loop of edges comes back onto its face beyond a seam
//! elsewhere on the loop, after its end face has trimmed it. A first or last run is also past an
//! end where the filleted edge is closed and the run lies beyond the plane on which the
//! corner at its vertex cuts the rail ([`past_closed_corner`], #2498): the fork takes the
//! top rail of a teardrop's rim, which crosses the rim itself just past the tip, for a
//! cliff overflow. As the fork's `8ba4667` at rev `68fb9162` (WR-631), the check also
//! tells whether a rail rolls over a tangent edge and whether a rail that leaves its
//! face runs on into the body's material ([`RailsFit`], `IsConcaveEdge`,
//! `sm_InsideMaterial`), for the executive's choice between a refusal and a cliff
//! overflow (a convex blend built over a cliff, the material it removes taken from the
//! body: `crate::blend::fillet_cliff_overflow`). The
//! fork's `IsKnifeEdge` ([`is_knife_edge`], `e418750`, WR-665) is here too, used by
//! `SmFilletCorner::Create`. The
//! nearest-edge search of `sm_InsideMaterial` keeps the first of equally near edges, in
//! the face's edge order, as the fork's strict `<` does. A closed rail's isolated contact
//! with a sharp rim is reported separately: keeping both loops would intersect them
//! (#20282). There is no recursion.

use super::{RailClassificationContext, LENT_CLASSIFICATION, OP};
use crate::blend::fillet_classification::FilletClassObject;
use crate::blend::fillet_geom::p1::{FilletBrep, FilletGeomType};
use crate::blend::fillet_solver::p1::{
    test_manifold_tangent_rollover_at, FilletSolver, FilletSolverOps,
};
use crate::curve::curve::Curve;
use crate::error::blend::{BlendFailure, CrossingInput, FilletSetUp, SolverFailure};
use crate::error::{EntityRef, Failure, Result, SolidError, SM_ERR_NULL_POINTER};
use crate::geom::extent1d::Extent1d;
use crate::geom::global_solver::SolverOperation;
use crate::intersect::curve_class::p1::{CurveClassification, CurveInterval};
use crate::intersect::curve_class::p2::ResetOptions;
use crate::intersect::point_class::{PointClassType, PointClassification};
use crate::kernel::Kernel;
use crate::math::Point3;
use crate::math::BIG_DOUBLE;
use crate::topology::edgeuse::p2 as eu2;
use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use crate::topology::face::p4 as face_p4;
use crate::topology::{edge, edgeuse::p1 as eu, Brep, EdgeId, FaceId};

/// What [`FilletSolver::rails_fit_their_faces_judged_with_ops`] finds of a solver's rails
/// (the fork's `RailsFitTheirFaces(rbFit, rbRollsOver, rbIntoMaterial)`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // the fork's out-flags and the same runs' wrap/material judgements
pub struct RailsFit {
    /// `rbFit`: every rail stays on its face (`false` = a rail leaves its face between
    /// the filleted edge's ends, or is nowhere on it).
    pub fit: bool,
    /// A closed rail stays on its face but touches a sharp boundary at isolated points.
    /// Keeping both loops would make them intersect, rather than bound a face.
    pub touches_rim: bool,
    /// `rbRollsOver`: a rail rolls over a tangent edge onto the next face.
    pub rolls_over: bool,
    /// `rbIntoMaterial`: a rail that leaves its face runs on inside the body (judged only
    /// when asked; `false` otherwise).
    pub into_material: bool,
    /// Every judged exit and return is locally concave at its classified parameter.
    /// False when material was not judged or a boundary was not classified.
    pub boundaries_into_material: bool,
    /// Not upstream (#10876): for each run of a rail off its face that makes `fit` false,
    /// the edge it leaves the face across (`None` where it leaves at no classified edge, or
    /// a rail nowhere on its face), in the order found: where the blend overflows, for the
    /// blend corner's Roll-over's second pass to tell an overflow across the cliff at V.
    pub left_across: Vec<Option<EdgeId>>,
    /// The classified return boundary of each judged off-face run between two on-face
    /// stretches. Kept separate so the corner Roll-over retains its exit-only contract.
    pub return_across: Vec<Option<EdgeId>>,
    /// Rail crossings handed to a planar cliff joined to the next blend span.
    pub continued_cliffs: Vec<(EdgeId, FaceId)>,
    /// Not the fork's (tangency-labs/solidcore#2175): a closed rail leaves its face past
    /// one end of the filleted edge and comes back onto it past the other, round the far
    /// side of the rail. An open edge's face is smaller than the blend beyond both ends;
    /// on a closed edge the labels instead refer to its periodic seam. Only a certified
    /// concave crash on a closed edge may trim such a run at both boundaries.
    pub wraps_past_both_ends: bool,
}

/// Compare the adjacent face-use normals at this boundary contact, not elsewhere
/// along the edge: a curved boundary can be smooth at one point and sharp at another.
fn rim_is_smooth_at(
    k: &Kernel,
    target: &mut Brep,
    e: EdgeId,
    parameter: f64,
    tangency_tol: f64,
) -> Result<bool> {
    let primary = eu::primary_edgeuse(target, e)?.ok_or_else(|| {
        Failure::new(
            OP,
            BlendFailure::Defect {
                why: "the rail classifier identified a solid rim without the primary edge use \
                      that the kernel's solid topology must retain",
            },
        )
        .with_status(SM_ERR_NULL_POINTER)
        .into_error()
    })?;
    let radial = eu::radial(target, primary)?.ok_or_else(|| {
        Failure::new(
            OP,
            BlendFailure::Defect {
                why: "the rail classifier identified a solid rim without the radial partner \
                      required to bound its material sector",
            },
        )
        .with_status(SM_ERR_NULL_POINTER)
        .into_error()
    })?;
    let options = BinormalOptions::default();
    let first = evaluate_binormal(k, target, primary, parameter, false, &options)?.face_normal;
    let second = evaluate_binormal(k, target, radial, parameter, false, &options)?.face_normal;
    Ok(first.angle_between(second)? <= tangency_tol)
}

/// A closed rail touching a sharp rim without crossing it leaves two touching loops.
/// Classification includes the modelling tolerance. A rail wholly on a boundary, a
/// smooth seam, or one crossing the rim is handled by the existing trim/overflow paths.
fn closed_rail_touches_rim(
    k: &Kernel,
    target: &mut Brep,
    face: FaceId,
    filleted: EdgeId,
    intervals: &[CurveInterval<FilletClassObject>],
    tangency_tol: f64,
) -> Result<bool> {
    if !edge::is_closed(target, filleted)?
        || !intervals
            .iter()
            .any(|i| i.mid.point_class() == PointClassType::Face)
        || intervals
            .iter()
            .any(|i| i.mid.point_class() == PointClassType::Unknown)
    {
        return Ok(false);
    }
    let mut previous = intervals.last();
    for ivl in intervals {
        let before = previous;
        previous = Some(ivl);
        let Some(before) = before else {
            continue;
        };
        // Only an isolated contact: a stretch along the rim has an Edge midpoint
        // and belongs to the existing trim path.
        if before.mid.point_class() != PointClassType::Face
            || ivl.mid.point_class() != PointClassType::Face
        {
            continue;
        }
        for point in [&before.end, &ivl.start] {
            if !matches!(
                point.point_class(),
                PointClassType::Edge | PointClassType::Vertex
            ) {
                continue;
            }
            let Some(object) = point.object() else {
                continue;
            };
            let edges = match (point.point_class(), object.target_entity()?) {
                (PointClassType::Edge, EntityRef::Edge(e)) => {
                    if !point.are_parameters_set() {
                        return Err(Failure::new(
                            OP,
                            BlendFailure::Defect {
                                why: "the rail classifier reported an edge contact without \
                                      locating its parameter, which classification must supply",
                            },
                        )
                        .with_status(SM_ERR_NULL_POINTER)
                        .into_error());
                    }
                    // The classifier has already located the contact on this edge.
                    vec![(e, point.t_param())]
                }
                (PointClassType::Vertex, EntityRef::Vertex(v)) => {
                    let mut incident = Vec::new();
                    for e in face_p4::edges(target, face, None, false, None)? {
                        let (a, b) = edge::start_end_vertices(target, e)?;
                        let interval = edge::interval(target, e)?;
                        if a == Some(v) {
                            incident.push((e, interval.min()));
                        }
                        // At a closed edge's seam, check both one-sided endpoints.
                        if b == Some(v) {
                            incident.push((e, interval.max()));
                        }
                    }
                    incident
                }
                _ => continue,
            };
            for (e, parameter) in edges {
                if e != filleted
                    && !edge::face_on_both_sides(target, e, face)?
                    && !rim_is_smooth_at(k, target, e, parameter, tangency_tol)?
                {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
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
        Err(e)
            if matches!(
                e,
                SolidError::Unported { .. } | SolidError::Cancelled { .. }
            ) || e.is_not_certified() =>
        {
            return Err(e);
        }
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
    match material_use(target, edge_id)? {
        Some(use_) => eu2::is_convex_radial_sector(k, target, use_, 10, false, 0.0),
        None => Err(SolidError::blend_solver(
            OP,
            SolverFailure::EdgeBoundsNoMaterial { edge: edge_id },
        )),
    }
}

/// The material-side radial-sector test at an actual crossing, rather than the
/// whole-edge samples: a spline seam can change convexity along its length.
fn is_concave_edge_at(
    k: &Kernel,
    target: &mut Brep,
    edge_id: EdgeId,
    parameter: f64,
) -> Result<bool> {
    if !edge::is_manifold(target, edge_id)? {
        return Ok(false);
    }
    let Some(use_) = material_use(target, edge_id)? else {
        return Ok(false);
    };
    let radial = eu::radial(target, use_)?.ok_or_else(|| {
        SolidError::blend_crossing_input_missing(OP, CrossingInput::MaterialEdgeUseRadial)
    })?;
    let options = eu2::StepOffOptions::default();
    let first = eu2::evaluate_binormal_step_off(k, target, use_, parameter, 0.0, &options)?;
    let second = eu2::evaluate_binormal_step_off(k, target, radial, parameter, 0.0, &options)?;
    // The same sign criterion as IsConvexRadialSector at zero angular tolerance.
    concave_from_dots(
        second.face_normal.dot(first.binormal),
        first.face_normal.dot(second.binormal),
    )
}

/// The sign test of [`is_concave_edge_at`] on its two dot products. A non-finite one is
/// refused, not classified as concave.
///
/// # Errors
///
/// [`BlendFailure::CrossingNotFinite`] for a non-finite dot product.
pub(super) fn concave_from_dots(first_dot: f64, second_dot: f64) -> Result<bool> {
    if !first_dot.is_finite() || !second_dot.is_finite() {
        return Err(SolidError::blend(OP, BlendFailure::CrossingNotFinite));
    }
    Ok(!(first_dot > -crate::math::EFF_ZERO && second_dot > -crate::math::EFF_ZERO))
}

/// Whether the edge bounds material: a use of it in a loop of a shell of a region that is
/// not a void (where [`is_concave_edge`] judges it). A sheet's edges bound none.
///
/// # Errors
///
/// Those of the topology queries.
pub fn edge_bounds_material(target: &Brep, edge_id: EdgeId) -> Result<bool> {
    Ok(material_use(target, edge_id)?.is_some())
}

/// [`edge_bounds_material`], but a loop edge use without a shell, or in a shell without a
/// region, is refused rather than taken for "no material" when no use bounds material: a
/// sheet's edges (loop uses in void regions, or no loop uses) bound none, a broken
/// ownership link is a failure. It refuses exactly the edges [`is_concave_edge`] refuses
/// and a sheet does not explain (#19829).
///
/// # Errors
///
/// [`SolverFailure::EdgeBoundsNoMaterial`] (as [`is_concave_edge`]) for an edge with no
/// material use and a loop use without a shell or region; those of the topology queries.
pub fn edge_bounds_material_linked(target: &Brep, edge_id: EdgeId) -> Result<bool> {
    if material_use(target, edge_id)?.is_some() {
        return Ok(true);
    }
    for use_ in edge::edgeuses(target, edge_id, None, None)? {
        if !eu::is_loop_edgeuse(target, use_)? {
            continue;
        }
        let linked = match eu::shell(target, use_)? {
            Some(shell) => crate::topology::shell::shell_region(target, shell)?.is_some(),
            None => false,
        };
        if !linked {
            return Err(SolidError::blend_solver(
                OP,
                SolverFailure::EdgeBoundsNoMaterial { edge: edge_id },
            ));
        }
    }
    Ok(false)
}

/// The first use of the edge in a loop of a shell of a region that is not a void.
fn material_use(target: &Brep, edge_id: EdgeId) -> Result<Option<crate::topology::EdgeuseId>> {
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
        return Ok(Some(use_));
    }
    Ok(None)
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
    let nearest = nearest
        .ok_or_else(|| SolidError::blend_solver(OP, SolverFailure::PointDropsOnNoFaceEdge))?;
    is_concave_edge(k, target, nearest)
}

/// The edge the solver fillets.
fn filleted_edge(solver: &FilletSolver, target: &Brep) -> Result<EdgeId> {
    let fil_eu = solver.edgeuse(0)?.ok_or_else(|| {
        SolidError::blend_solver(OP, SolverFailure::SolverEdgeUseMissing { index: 0 })
    })?;
    eu::edge(target, fil_eu)?.ok_or_else(|| {
        SolidError::blend_at(
            OP,
            BlendFailure::EdgeUseWithoutEdge,
            EntityRef::Edgeuse(fil_eu),
        )
    })
}

/// The filleted edge's curve.
fn filleted_curve(target: &Brep, fil_edge: EdgeId) -> Result<&Curve> {
    edge::curve(target, fil_edge)?.ok_or_else(|| {
        SolidError::blend_at(
            OP,
            BlendFailure::EdgeWithoutCurve,
            EntityRef::Edge(fil_edge),
        )
    })
}

/// The filleted edge's point and tangent at the start and at the end of its interval, each
/// taken from the edge's own side.
fn end_jets(k: &Kernel, curve: &Curve, ivl: &Extent1d) -> Result<[(Point3, Point3); 2]> {
    let at_start = curve.evaluate(k, ivl.min(), 1, false, true)?;
    let at_end = curve.evaluate(k, ivl.max(), 1, true, true)?;
    let (Some(&p0), Some(&t0), Some(&p1), Some(&t1)) = (
        at_start.first(),
        at_start.get(1),
        at_end.first(),
        at_end.get(1),
    ) else {
        return Err(SolidError::blend_null_defect(OP, JET_SHORT));
    };
    Ok([(p0, t0), (p1, t1)])
}

/// The rail's point and tangent at `rail_param`, from the left or from the right.
fn rail_jet(
    k: &Kernel,
    rail: &Curve,
    rail_param: f64,
    from_left: bool,
) -> Result<(Point3, Point3)> {
    let on_rail = rail.evaluate(k, rail_param, 1, from_left, true)?;
    let (Some(&point), Some(&heading)) = (on_rail.first(), on_rail.get(1)) else {
        return Err(SolidError::blend_null_defect(OP, JET_SHORT));
    };
    Ok((point, heading))
}

/// A curve evaluated with one derivative returned fewer than two values
/// (`SolidError::blend_null_defect`).
const JET_SHORT: &str = "a curve evaluated with one derivative returned fewer than two values: \
                         `Curve::evaluate` returns the point and each derivative asked for";

/// Whether the run off the support leaves through the rim of the planar cliff
/// of a neighboring span, on the cliff's outward side. The joined corner does
/// not cut off that interval: the cliff trims the full continuous blend.
fn continued_cliff_crossing(
    solver: &FilletSolver,
    k: &Kernel,
    target: &Brep,
    context: &impl RailClassificationContext,
    rail: &Curve,
    at: &PointClassification<FilletClassObject>,
    outside_param: f64,
) -> Result<Option<(EdgeId, FaceId)>> {
    let (PointClassType::Edge, Some(object)) = (at.point_class(), at.object()) else {
        return Ok(None);
    };
    let EntityRef::Edge(rim) = object.target_entity()? else {
        return Ok(None);
    };
    let source = filleted_edge(solver, target)?;
    let (first, last) = edge::start_end_vertices(target, source)?;
    for end in [first, last].into_iter().flatten() {
        let Some(wall) = context.continued_cliff(end, rim) else {
            continue;
        };
        let (origin, normal) = crate::blend::fillet_overflow::outward_plane(k, target, wall)?;
        let point = rail.evaluate_point(k, outside_param)?;
        if (point - origin).dot(normal) > solver.approx_tol_3d {
            return Ok(Some((rim, wall)));
        }
    }
    Ok(None)
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
    let fil_edge = filleted_edge(solver, target)?;
    // `NER(pEdgeCurve)` before either test
    filleted_curve(target, fil_edge)?;

    // Across or through the face's side edge at an end.
    if !side_edge_ends(target, fil_edge, face, leave)?.is_empty() {
        return Ok(true);
    }

    // On or beyond the cross section at an end.
    Ok(beyond_end_section(solver, k, target, rail, rail_param)?.is_some())
}

/// The ends of the filleted edge (0 its start vertex, 1 its end vertex) whose side edge in
/// `face`'s loop (the edge before or after the filleted edge there) the rail crosses at
/// `at`, across it or through one of its vertices: none where `at` is on no side edge, both
/// where the side edge joins them (a loop of two edges).
fn side_edge_ends(
    target: &Brep,
    fil_edge: EdgeId,
    face: FaceId,
    at: &PointClassification<FilletClassObject>,
) -> Result<Vec<usize>> {
    let (fil_start, fil_end) = edge::start_end_vertices(target, fil_edge)?;
    let (leave_edge, leave_vertex) = match (at.point_class(), at.object()) {
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
            let (first, second) = edge::start_end_vertices(target, side)?;
            let crosses = leave_edge == Some(side)
                || leave_vertex.is_some_and(|v| first == Some(v) || second == Some(v));
            if !crosses {
                continue;
            }
            return Ok([fil_start, fil_end]
                .into_iter()
                .enumerate()
                .filter(|(_, v)| v.is_some() && (*v == first || *v == second))
                .map(|(end, _)| end)
                .collect());
        }
    }
    Ok(Vec::new())
}

/// The end of the filleted edge past which a rail crosses its face's side edge at that end
/// (at rail parameter `rail_param`, classified `at`), if it does: `leaving` the face, heading
/// out past the end; else coming back onto it, heading in from beyond the end. On a loop of
/// two edges, where the side edge joins both ends, the crossing is at the end nearer it.
///
/// A rail that crosses its face's side edge at an end has reached the end face there, where
/// the corner at that end trims the blend. A rail run on round a closed loop of edges can
/// come back onto its face beyond a seam elsewhere on the loop, so its run off the face
/// across the side edge is not a first or last one; and where the end face slopes back over
/// the face, the crossing lies before the end's cross section, so the cross section test
/// alone takes it for a rail leaving beside the edge.
#[allow(clippy::too_many_arguments)] // the run's crossing, as `leaves_past_an_end` takes it
fn past_end_through_side_edge(
    k: &Kernel,
    target: &Brep,
    fil_edge: EdgeId,
    face: FaceId,
    rail: &Curve,
    rail_param: f64,
    at: &PointClassification<FilletClassObject>,
    leaving: bool,
) -> Result<Option<usize>> {
    let ends = side_edge_ends(target, fil_edge, face, at)?;
    if ends.is_empty() {
        return Ok(None);
    }
    let curve = filleted_curve(target, fil_edge)?;
    let jets = end_jets(k, curve, &edge::interval(target, fil_edge)?)?;
    // the side it leaves toward, or comes from
    let (point, heading) = rail_jet(k, rail, rail_param, leaving)?;
    Ok(crossing_end(&ends, jets, point, heading, leaving))
}

/// Of the ends `ends` (0 the start, 1 the end) of a filleted edge whose point and tangent
/// there are `jets`, the one a rail crossing a side edge at `point`, heading `heading`, is
/// past: `leaving` its face heading out past that end, else coming back heading in from
/// beyond it. Where the side edge joins both ends (a loop of two edges), the crossing is at
/// the nearer one, and only that end is judged.
pub(super) fn crossing_end(
    ends: &[usize],
    jets: [(Point3, Point3); 2],
    point: Point3,
    heading: Point3,
    leaving: bool,
) -> Option<usize> {
    let [(start, start_tangent), (end, end_tangent)] = jets;
    let nearer = if ends.len() > 1 {
        usize::from(point.distance(end) < point.distance(start))
    } else {
        *ends.first()?
    };
    // the edge's tangent at that end, pointing out past it
    let ahead = if nearer == 0 {
        -heading.dot(start_tangent)
    } else {
        heading.dot(end_tangent)
    };
    ((leaving && ahead > 0.0) || (!leaving && ahead < 0.0)).then_some(nearer)
}

/// The end of the filleted edge a run of a rail off its face between two stretches on it
/// lies past, through the face's side edge at that end ([`past_end_through_side_edge`]) or
/// on or beyond the end's cross section ([`beyond_end_section`]): `leaving` at the run's
/// first point, else coming back at its last.
#[allow(clippy::too_many_arguments)] // the run's crossing, as `leaves_past_an_end` takes it
fn run_end_past(
    solver: &FilletSolver,
    k: &Kernel,
    target: &Brep,
    face: FaceId,
    rail: &Curve,
    rail_param: f64,
    at: &PointClassification<FilletClassObject>,
    leaving: bool,
) -> Result<Option<usize>> {
    let fil_edge = filleted_edge(solver, target)?;
    match past_end_through_side_edge(k, target, fil_edge, face, rail, rail_param, at, leaving)? {
        Some(end) => Ok(Some(end)),
        None => beyond_end_section(solver, k, target, rail, rail_param),
    }
}

/// Whether a first or last run of a rail off its face (`entering`: the first, which comes
/// onto the face at `rail_param`; else the last, which leaves it there) lies past the
/// corner where a closed filleted edge meets itself (not the fork's, tangency-labs/solidcore#2498).
///
/// Both ends of a closed edge are its one vertex V, and the corner there cuts each end's
/// blend where it meets the other's, on the plane through V bisecting the edge's end
/// tangents T0 (at its start) and T1 (at its end), of normal T0 + T1: the start's blend on
/// its positive side, the end's on its negative side (for a G1 closure, the cross section
/// at V). A rail runs from beyond that plane into its own blend's side, so a run lies past
/// the corner when the rail, where it comes onto the face, is still to cross the plane
/// ahead of it (or, where it leaves, has crossed it behind it). A rail of one end's blend
/// that reaches the other end's stretch of the edge before the plane (a convex corner at
/// V of interior angle a, the rail meeting the edge's other stretch r / sin a from V) leaves
/// the face across the filleted edge itself, which `LeavesPastAnEnd` takes for a rail
/// beside the edge: its face's loop has no side edge at V, and the edge point nearest
/// where it leaves is that point of the edge, not an end.
fn past_closed_corner(
    solver: &FilletSolver,
    k: &Kernel,
    target: &Brep,
    rail: &Curve,
    rail_param: f64,
    entering: bool,
) -> Result<bool> {
    let fil_edge = filleted_edge(solver, target)?;
    if edge::start_vertex(target, fil_edge)? != edge::end_vertex(target, fil_edge)? {
        return Ok(false);
    }
    let curve = filleted_curve(target, fil_edge)?;
    let [(vertex, t0), (_, t1)] = end_jets(k, curve, &edge::interval(target, fil_edge)?)?;
    let normal = t0.unitize()? + t1.unitize()?;
    let (point, heading) = rail_jet(k, rail, rail_param, !entering)?;
    let side = (point - vertex).dot(normal);
    let ahead = heading.dot(normal);
    Ok(if entering {
        side * ahead < 0.0
    } else {
        side * ahead > 0.0
    })
}

/// The second test of `SmFilletSolver::LeavesPastAnEnd`: the end of the filleted edge (0
/// its interval's start, 1 its end) on or beyond whose cross section the rail's point at
/// `rail_param` lies (the edge point nearest it is that end, within the solver's 3D
/// tolerance), if any.
fn beyond_end_section(
    solver: &FilletSolver,
    k: &Kernel,
    target: &Brep,
    rail: &Curve,
    rail_param: f64,
) -> Result<Option<usize>> {
    let fil_edge = filleted_edge(solver, target)?;
    let edge_curve = filleted_curve(target, fil_edge)?;
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
        return Err(SolidError::blend_solver(
            OP,
            SolverFailure::LeavePointDoesNotDropOnEdge,
        ));
    }
    let foot = edge_curve.evaluate_point(k, dropped.parameter)?;
    // Not the fork's (#16654): the drop may stop at the far end of the edge, a local minimum
    // (a point past the end of an arc of more than a half turn is nearest that end, yet moving
    // in from the other end first takes it further away): an end nearer the point than the
    // foot found is the nearest point of the edge.
    let reach = foot.distance(leave_point);
    let mut nearest: Option<(usize, f64)> = None;
    for (index, end) in [ivl.min(), ivl.max()].into_iter().enumerate() {
        let away = edge_curve.evaluate_point(k, end)?.distance(leave_point);
        if away < reach - solver.approx_tol_3d && nearest.is_none_or(|(_, d)| away < d) {
            nearest = Some((index, away));
        }
    }
    if let Some((index, _)) = nearest {
        return Ok(Some(index));
    }
    for (index, end) in [ivl.min(), ivl.max()].into_iter().enumerate() {
        if foot.distance(edge_curve.evaluate_point(k, end)?) <= solver.approx_tol_3d {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

/// The curve and interval of the solver's filleted edge (that of its first edge use).
fn filleted_edge_curve<'b>(
    solver: &FilletSolver,
    target: &'b Brep,
) -> Result<(&'b Curve, Extent1d)> {
    let fil_edge = filleted_edge(solver, target)?;
    Ok((
        filleted_curve(target, fil_edge)?,
        edge::interval(target, fil_edge)?,
    ))
}

/// The end of the filleted edge (0 its interval's start, 1 its end) whose point the rail's
/// point at `rail_param` is, within the solver's 3D tolerance, if any: where the round's
/// cross section has shrunk to the edge's end vertex.
fn at_end_vertex(
    solver: &FilletSolver,
    k: &Kernel,
    target: &Brep,
    rail: &Curve,
    rail_param: f64,
) -> Result<Option<usize>> {
    let (edge_curve, ivl) = filleted_edge_curve(solver, target)?;
    let point = rail.evaluate_point(k, rail_param)?;
    for (index, end) in [ivl.min(), ivl.max()].into_iter().enumerate() {
        if point.distance(edge_curve.evaluate_point(k, end)?) <= solver.approx_tol_3d {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

impl FilletSolver {
    /// `SmFilletSolver::RailsFitTheirFaces` (the fork's refusal, see the module doc):
    /// whether every rail of this solver's fillet, untrimmed, stays on the face it rolls
    /// on. Each rail is classified against its face. It may run off the face past the
    /// filleted edge's ends (`leaves_past_an_end`) and off across a tangent side edge (a
    /// rollover onto the next face). A rail that is nowhere on its face, or that leaves it
    /// between its ends across an edge that is not tangent (a wall, an opening's rim)
    /// means the blend is larger than the faces: `false`. Touching or running along the
    /// face's boundary is on the face. An isolated contact of a closed rail with a sharp
    /// rim is also reported in [`RailsFit::touches_rim`]: keeping both loops is invalid.
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
        let mut touches_rim = false;
        let mut rolls_over = false;
        let mut into_material = false;
        let mut boundaries_into_material = judge_material;
        let mut left_across = Vec::new();
        let mut return_across = Vec::new();
        let mut continued_cliffs = Vec::new();
        let mut wraps_past_both_ends = false;
        // the filleted edge (a rail that leaves its face across it is not leaving past a cliff)
        let filleted = match solver.solver().edgeuse(0)? {
            Some(u) => eu::edge(target, u)?,
            None => None,
        };
        // A sheet's edge bounds no material: a rail that runs off a sheet's face is cut by the
        // corners and the face's boundary as upstream cuts it, with no cliff to overflow and
        // no body to subtract from (not the fork's, whose `IsConcaveEdge` fails there).
        if let Some(e) = filleted {
            if !edge_bounds_material(target, e)? {
                return Ok(RailsFit {
                    fit: true,
                    touches_rim: false,
                    rolls_over: false,
                    into_material: false,
                    boundaries_into_material: false,
                    left_across: Vec::new(),
                    return_across: Vec::new(),
                    continued_cliffs: Vec::new(),
                    wraps_past_both_ends: false,
                });
            }
        }
        for id in solver.solver().fillet_geoms() {
            if fillets.geom(id)?.fillet_geom_type() != FilletGeomType::Default {
                continue;
            }
            for ii in 0..2 {
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
                let intervals = intervals
                    .ok_or_else(|| SolidError::blend_null_defect(OP, LENT_CLASSIFICATION))?;
                if let Some(e) = filleted {
                    let tolerance = solver.solver().approx_tol_3d;
                    let closed = curve.is_closed(k, &curve.natural_interval(), tolerance)?;
                    touches_rim |= closed
                        && closed_rail_touches_rim(
                            k,
                            target,
                            face,
                            e,
                            &intervals,
                            solver.solver().tangency_tolerance,
                        )?;
                }

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
                    let last = intervals.get(j_last).ok_or_else(|| {
                        SolidError::blend_null_defect(
                            OP,
                            "the run off the face ends past the classification's last \
                             interval: `j_last` advances only while the next interval exists",
                        )
                    })?;
                    // Where the rail leaves the face: the first interval ends there, the
                    // others start.
                    let (leave, d_leave) = if j == 0 {
                        (&last.end, last.interval.max())
                    } else {
                        (&ivl.start, ivl.interval.min())
                    };
                    // this run leaves past one end and comes back past the other
                    let mut wraps = false;
                    let past_end = |point: &PointClassification<FilletClassObject>, d: f64| {
                        leaves_past_an_end(solver.solver(), k, target, face, &curve, d, point)
                    };
                    if j == 0 || j_last + 1 == n {
                        if let Some((rim, wall)) = continued_cliff_crossing(
                            solver.solver(),
                            k,
                            target,
                            context,
                            &curve,
                            leave,
                            ivl.interval.evaluate(0.5),
                        )? {
                            continued_cliffs.push((rim, wall));
                            continue;
                        }
                        if past_end(leave, d_leave)?
                            || past_closed_corner(
                                solver.solver(),
                                k,
                                target,
                                &curve,
                                d_leave,
                                j == 0,
                            )?
                        {
                            continue;
                        }
                    } else if let Some(end) = run_end_past(
                        solver.solver(),
                        k,
                        target,
                        face,
                        &curve,
                        d_leave,
                        leave,
                        true,
                    )? {
                        // A run between two stretches on the face that lies wholly past an
                        // end (it leaves and comes back past that end: through the face's
                        // side edge there, or on or beyond its cross section) is trimmed by
                        // the corner there too (tangency-labs/solidcore#1580; the fork's
                        // `518566f` tried only the first and last runs). A run that leaves
                        // past one end and comes back past the other runs round the far side
                        // of a closed rail, and is judged as the fork judges it
                        // (tangency-labs/solidcore#2175).
                        let back = run_end_past(
                            solver.solver(),
                            k,
                            target,
                            face,
                            &curve,
                            last.interval.max(),
                            &last.end,
                            false,
                        )?;
                        if back == Some(end) {
                            continue;
                        }
                        // A run that leaves (or comes back) through an end's vertex itself, where
                        // the round's faces turn tangent and its cross section shrinks to that
                        // vertex (two tubes of one radius), and comes back (or leaves) past the
                        // other end lies past the ends: past that vertex the rail is the
                        // round's run on, which the ends trim (#16654).
                        let d_back = last.interval.max();
                        let pinched_leave =
                            at_end_vertex(solver.solver(), k, target, &curve, d_leave)?;
                        let pinched_back =
                            at_end_vertex(solver.solver(), k, target, &curve, d_back)?;
                        if (pinched_leave == Some(end) && back.is_some())
                            || back.is_some_and(|b| pinched_back == Some(b))
                        {
                            continue;
                        }
                        wraps = back.is_some();
                    }
                    let side = match (leave.point_class(), leave.object()) {
                        (PointClassType::Edge, Some(o)) => match o.target_entity()? {
                            EntityRef::Edge(e) => Some(e),
                            _ => None,
                        },
                        _ => None,
                    };
                    // A rail that leaves its face across the filleted edge itself crosses
                    // where the two faces meet, not a cliff: the faces turn tangent there
                    // (the blend's section closes up at an end), and the corner at that end
                    // trims the fillet as upstream does. No overflow construction applies.
                    if side.is_some() && side == filleted {
                        continue;
                    }
                    if let Some(side) = side {
                        let tolerance = solver.rollover_tolerance(k, target)?;
                        if test_manifold_tangent_rollover_at(
                            k,
                            target,
                            side,
                            leave.t_param(),
                            tolerance,
                        )?
                        .0
                        {
                            rolls_over = true;
                            continue; // a rollover onto the next face
                        }
                    }
                    fit = false;
                    left_across.push(side);
                    let leaves_into_material = judge_material
                        && match side {
                            Some(edge) => is_concave_edge_at(k, target, edge, leave.t_param())?,
                            None => false,
                        };
                    boundaries_into_material &= leaves_into_material;
                    if j > 0 && j_last + 1 < n {
                        let returns_across = match (last.end.point_class(), last.end.object()) {
                            (PointClassType::Edge, Some(object)) => match object.target_entity()? {
                                EntityRef::Edge(edge) => Some(edge),
                                _ => None,
                            },
                            _ => None,
                        };
                        boundaries_into_material &= judge_material
                            && match returns_across {
                                Some(edge) => {
                                    is_concave_edge_at(k, target, edge, last.end.t_param())?
                                }
                                None => false,
                            };
                        return_across.push(returns_across);
                    }
                    // only a run that is itself judged off its face (not one passed over
                    // above as trimmed, tangent at the filleted edge or rolled over)
                    if wraps {
                        wraps_past_both_ends = true;
                    }
                    if judge_material {
                        // the edge it leaves across (or the face's edge nearest where it leaves)
                        let inside = if side.is_some() {
                            leaves_into_material
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
                    boundaries_into_material = false;
                    left_across.push(None);
                    if judge_material {
                        // the rail's point nearest the middle of the filleted edge
                        let fil_edge = filleted_edge(solver.solver(), target)?;
                        let fil_curve = filleted_curve(target, fil_edge)?;
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
                            return Err(SolidError::blend_solver(
                                OP,
                                SolverFailure::EdgeMiddleDoesNotDropOnRail,
                            ));
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
            touches_rim,
            rolls_over,
            into_material,
            boundaries_into_material,
            left_across,
            return_across,
            continued_cliffs,
            wraps_past_both_ends,
        })
    }
}
