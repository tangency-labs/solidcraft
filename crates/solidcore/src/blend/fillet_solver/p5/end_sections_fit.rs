//! `SmFilletSolver::EndSectionsFitTheirFaces`: whether a fillet's end section at each
//! Nx1 end lies on its end face.
//!
//! Not upstream (SMLib 1.0.1 has none): the SolidCraft fork's tangency-labs/smlib
//! `a307480`, used only with the opt-in `Kernel::fixes.fillet_cliff_overflow` (WR-631),
//! in the executive's cliff-overflow choice: an end section that runs off its end face
//! under material overflows it, and the blend is built as a cliff overflow is.
//!
//! Deviations: the fork's `sm_SrfSrfIntersection` takes two `SmFilletVertex` objects
//! holding the crossings; [`srf_srf_intersection`] takes the points. The section, a curve
//! in no body, is classified through [`RailClassificationContext::classify_free_curve`]
//! (`SmFace::CurveOnClassify`). There is no recursion.

use super::{RailClassificationContext, OP};
use crate::blend::fillet_geom::p1::{FilletBrep, FilletCornerType, FilletGeomType};
use crate::blend::fillet_geom::p2::{srf_srf_intersection, SrfSrfOptions};
use crate::blend::fillet_solver::p1::{
    test_manifold_tangent_rollover, FilletSolver, FilletSolverOps,
};
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::global_solver::SolverOperation;
use crate::intersect::curve_class::p1::CurveClassification;
use crate::intersect::curve_class::p2::ResetOptions;
use crate::intersect::point_class::PointClassType;
use crate::kernel::Kernel;
use crate::math::{Point3, BIG_DOUBLE};
use crate::topology::{edge, edgeuse::p1 as eu, vertex, Brep, FaceId};

use super::rails_fit::is_concave_edge;

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
    /// fillet overflows its end face. A section off E across a convex edge (a hole's rim,
    /// a cliff) is not judged here. The section is the surfaces' exact intersection
    /// between the two crossings, classified against E. Judged only where the corner
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
        let fil_eu = solver
            .solver()
            .edgeuse(0)?
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let fil_edge = eu::edge(target, fil_eu)?.ok_or_else(|| SolidError::null_pointer(OP))?;
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
                .ok_or_else(|| SolidError::null_pointer(OP))?
                .clone();
            let (start, end) = edge::start_end_vertices(target, fil_edge)?;
            for v in [start, end] {
                let v = v.ok_or_else(|| SolidError::null_pointer(OP))?;
                let corner = context.corner_of_vertex(v)?;
                let vertex_edges = vertex::edges(target, v, None)?;
                let is_nx1 = match corner {
                    Some(c) => context.corner_type(c)? == FilletCornerType::NByOne,
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
                    // the rail's crossing of the side edge nearest the vertex
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
                    let intervals = intervals.ok_or_else(|| SolidError::null_pointer(OP))?;
                    let mut nearest = BIG_DOUBLE;
                    for ivl in &intervals {
                        for (pc, t) in [
                            (&ivl.start, ivl.interval.min()),
                            (&ivl.end, ivl.interval.max()),
                        ] {
                            let on_side = pc.point_class() == PointClassType::Edge
                                && match pc.object() {
                                    Some(o) => {
                                        matches!(o.target_entity(), Ok(EntityRef::Edge(e)) if e == side)
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
                let section = curves
                    .curve_3d
                    .ok_or_else(|| SolidError::null_pointer(OP))?;
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
                    if !dropped.success || dropped.distance > 10.0 * approx_tol {
                        return Err(SolidError::sm_err(OP));
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
                    // Off the face across a wall's foot (a concave edge of the end face:
                    // something standing on it), the section runs under material. Across a
                    // convex edge (a hole's rim, a cliff) it runs into the open, which this
                    // does not judge.
                    let mut under = false;
                    for pc in [&ivl.start, &ivl.end] {
                        if pc.point_class() != PointClassType::Edge {
                            continue;
                        }
                        let Some(o) = pc.object() else {
                            continue;
                        };
                        let EntityRef::Edge(e) = o.target_entity()? else {
                            return Err(SolidError::null_pointer(OP));
                        };
                        if is_concave_edge(k, target, e)? {
                            under = true;
                        }
                    }
                    if under {
                        fit = false;
                    }
                }
            }
        }
        Ok(fit)
    }
}
