//! The fork's ease of a round into its curvature-continuous section before a setback.
//!
//! Upstream: the fork tangency-labs/smlib `e74bbd3` (`SmFilletExecutive::EaseSetBackSections`
//! and its helpers in `source/SMLib/src/SmFilletExecutive.cpp`, `SmFilletCorner::m_dEaseDist`
//! in `source/SMLib/inc/SmFilletCorner.h`); not in SMLib at the pin (WR-616,
//! tangency-labs/solidcore#1376).
//!
//! | Rust | Upstream (fork) |
//! |------|-----------------|
//! | [`FilletSession::set_corner_ease`](super::FilletSession::set_corner_ease) | `SmFilletCorner::SetEaseDist` |
//! | `SessionObjects::ease_set_back_sections` (private) | `SmFilletExecutive::EaseSetBackSections` |
//!
//! The ease itself (`SmEaseRamp`, `sm_EaseSurface` and its helpers) moved, unchanged, to
//! [`crate::blend::blend_setback`] (the fork's `SmBlendSetback::EaseSurface`, tangency-labs/smlib
//! `870c970`); the executive calls it there.
//!
//! Deviations: the ease is per corner and `0.0` by default, so a session that sets none runs
//! the pinned C++ (the executive's call finds no eased corner and changes nothing). The fork's
//! `SM_CAST_PTR(SmBSplineSurface, ..)` of the round's surface and of its rails, centre line and
//! UV curves are their B-spline parts (an analytic surface's with its NURB,
//! `GetOrCreateGwNurbPointer`); a kind the cast refuses is
//! [`BlendFailure::EaseGeometryNotBSpline`], status `SM_ERR_NULL_POINTER` (`NER`). The
//! orientation test of `sm_EaseSurface` evaluates that B-spline part where the fork calls the
//! copy's (analytic) `EvaluatePoint`; the rail drop onto a face's surface runs without the face's
//! surface cache. Every `SER(SM_ERR)` is `SM_ERR` and every `NER` `SM_ERR_NULL_POINTER`, as
//! typed causes (#19825): [`BlendFailure::EaseNotBuilt`] where the ease stops,
//! [`BlendFailure::EaseInputMissing`] for a missing input and a [`BlendFailure::Defect`] where
//! a thing the executive built is missing or a step that cannot fail did; the fork's checks
//! of a list's length followed by a read of its first element are one slice match. The rails
//! are read by one closure called for rail 0, then rail 1, where the fork loops over both.
//! Out-params are return values. No recursion.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // the fork's NaN-safe tests

use super::{live_solver_ops, SessionObjects};
use crate::blend::blend_setback::{ease_surface_of, interval_in, uv_at, EaseRamp};
use crate::blend::fillet_geom::p1::{FilletGeomId, FilletVertexType, SurfaceHandle};
use crate::blend::fillet_section::section::{GnFaces, GnSectionFsg};
use crate::blend::fillet_solver::p1::{FilletSolverOps, FilletSurfaceGeneratorType};
use crate::curve::curve::{Curve, CurveKind};
use crate::error::blend::{BlendFailure, EaseGeometry, EaseInput, EaseStop};
use crate::error::{EntityRef, Failure, Result, SolidError, SM_ERR_NULL_POINTER};
use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::tol::{approx_tol3d, zone_tol3d};
use crate::geom::vector2d::Point2;
use crate::intersect::surface_intersector::p1::{SrfIndex, TsectPnt};
use crate::kernel::Kernel;
use crate::math::EFF_ZERO;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::Surface;
use crate::topology::edgeuse::p1 as eu;
use crate::topology::face::p2 as face2;
use crate::topology::{edge, EdgeId, VertexId};

use super::FilletObject;

const OP: &str = "blend::fillet_executive::session::ease";

/// `SM_ERR`: the ease stops at `stop` ([`BlendFailure::EaseNotBuilt`]).
fn stop(stop: EaseStop) -> SolidError {
    SolidError::blend(OP, BlendFailure::EaseNotBuilt(stop))
}

/// `SM_ERR_NULL_POINTER` (`NER`): a thing the ease reads lacks `input`
/// ([`BlendFailure::EaseInputMissing`]).
fn missing(input: EaseInput) -> SolidError {
    SolidError::blend(OP, BlendFailure::EaseInputMissing(input))
}

/// [`missing`] at the entity `at`.
fn missing_at(input: EaseInput, at: EntityRef) -> SolidError {
    SolidError::blend_at(OP, BlendFailure::EaseInputMissing(input), at)
}

/// `SM_ERR_NULL_POINTER` (`NER` of the cast): `geometry` has no B-spline form
/// ([`BlendFailure::EaseGeometryNotBSpline`]).
fn not_bspline(geometry: EaseGeometry) -> SolidError {
    SolidError::blend(OP, BlendFailure::EaseGeometryNotBSpline(geometry))
}

/// A step that cannot fail did ([`BlendFailure::Defect`], status `SM_ERR`), `why` saying
/// why it is the kernel's fault.
fn defect(why: &'static str) -> SolidError {
    SolidError::blend(OP, BlendFailure::Defect { why })
}

/// A missing thing the executive built ([`BlendFailure::Defect`] keeping the upstream `NER`
/// status `SM_ERR_NULL_POINTER`), `why` saying why it is always there.
fn null_defect(why: &'static str) -> SolidError {
    Failure::new(OP, BlendFailure::Defect { why })
        .with_status(SM_ERR_NULL_POINTER)
        .into_error()
}

/// Why a round solver's edge uses and their edges are there.
const SOLVER_EDGES: &str = "a round solver lacks an edge use, or its edge use an edge: a \
     solver is set up on the two edge uses of the edge it rolls along";

/// Why a curve's evaluation gives its point and first derivative.
const EVALUATED: &str = "the filleted edge's curve evaluated with one derivative gives no \
     point or tangent: a curve's evaluation returns its point and each derivative asked for";

/// The B-spline part of a curve (`SM_CAST_PTR(SmBSplineCurve, ..)`): the kinds derived from
/// `SmBSplineCurve`; `NULL` (`None`) for the others.
fn bspline_curve(curve: &Curve) -> Option<&BSplineCurve> {
    match curve.kind() {
        CurveKind::BSpline(c) => Some(c),
        CurveKind::Line(c) => Some(c.base()),
        CurveKind::Ellipse(c) => Some(c.base()),
        CurveKind::Hyperbola(c) => Some(c.base()),
        CurveKind::Parabola(c) => Some(c.base()),
        _ => None,
    }
}

/// `FindParameterAtArcLength` under `SER` (`SM_ERR_NOT_CONVERGING` is an error), with the
/// edge's tolerance as the curve owner's.
fn arc_length_param(
    k: &Kernel,
    curve: &Curve,
    start: f64,
    length: f64,
    owner_zone_tol_3d: Option<f64>,
) -> Result<f64> {
    let r =
        curve.find_parameter_at_arc_length_with_zone_tol(k, start, length, owner_zone_tol_3d)?;
    if r.converged {
        Ok(r.end_param)
    } else {
        // the cause's status is `r.status()` of an unconverged search, `SM_ERR_NOT_CONVERGING`
        Err(SolidError::blend(
            OP,
            BlendFailure::ArcLengthNotConverged {
                start,
                length,
                stopped_at: r.end_param,
            },
        ))
    }
}

impl SessionObjects<'_> {
    /// `SmFilletExecutive::EaseSetBackSections` (the fork's, tangency-labs/smlib `e74bbd3`):
    /// once the fillet surfaces exist and before the corners' edges are computed, for each
    /// `SM_FV_SETBACK` vertex of a corner with an ease (in `corners`' order), the round's
    /// surface is replaced, over the rail parameter from where the plane square to the edge
    /// at the setback plus the ease cuts the rail to where the setback's section meets it,
    /// by the C2 blend of its own surface into the order-2 G^n section at thumbweight 0 on
    /// the same rails (the fork's 11ea2d9 and 22d258a; its first version, `e74bbd3`, took
    /// `SmBlendCurveCrossSectionFSG`'s `G2` at thirds). A round whose sections are already
    /// curvature-continuous (`SM_FSG_BLEND_CURVE`) keeps them; ramps on both ends of one round go into one
    /// blend. Without an eased corner nothing changes.
    ///
    /// # Errors
    /// `SM_ERR` and `SM_ERR_NULL_POINTER` where the fork `SER`s and `NER`s, as
    /// [`BlendFailure::EaseNotBuilt`], [`BlendFailure::EaseInputMissing`],
    /// [`BlendFailure::EaseGeometryNotBSpline`] or a [`BlendFailure::Defect`]; those of the
    /// solves, drops and surface constructions.
    #[allow(clippy::too_many_lines)] // one fork routine
    pub(super) fn ease_set_back_sections(
        &mut self,
        k: &mut Kernel,
        corners: &[FilletObject],
    ) -> Result<()> {
        let k: &Kernel = k;
        let mut geoms: Vec<FilletGeomId> = Vec::new();
        let mut ramps: Vec<Vec<EaseRamp>> = Vec::new();
        for &object in corners {
            let FilletObject::Corner(id) = object else {
                return Err(super::invalid("executive corner is not a session corner"));
            };
            let corner = self
                .corners
                .get(id.0 as usize)
                .ok_or_else(|| super::invalid("corner is not live in this session"))?;
            if !(corner.set_back_dist() > 0.0) || !(corner.ease_dist() > 0.0) {
                continue;
            }
            let cp_vertex: VertexId = corner
                .filleted_vertex()
                .ok_or_else(|| missing(EaseInput::CornerVertex))?;
            // the corner's setback along each of its edges (GetSetBackDistAlong), read while
            // the surfaces change
            let ease_dist = corner.ease_dist();
            let mut along_edge: Vec<(EdgeId, f64)> = Vec::new();
            for &eu in corner.all_eus.as_deref().unwrap_or_default() {
                if let Some(e) = eu::edge(self.target, eu)? {
                    along_edge.push((e, corner.set_back_dist_along(self.target, e)?));
                }
            }
            let set_back_dist = corner.set_back_dist();
            let mut fvs = Vec::new();
            corner.fillet_vertices(&mut fvs);
            for fv in fvs {
                let vertex = self.pseudo.fillet_vertex(fv)?;
                if vertex.fillet_vertex_type() != FilletVertexType::Setback {
                    continue;
                }
                if !vertex.is_processed() {
                    return Err(stop(EaseStop::SetbackVertexNotPlaced));
                }
                let class_object = vertex.point_class_object();
                let fv_geoms = self.pseudo.vertex_fillet_geoms(fv)?;
                let &[fg] = fv_geoms.as_slice() else {
                    return Err(stop(EaseStop::SetbackNotOnOneRound));
                };
                let solver_id = self
                    .pseudo
                    .geom(fg)?
                    .solver
                    .ok_or_else(|| missing(EaseInput::RoundSolver))?;
                let base = self.solver(solver_id)?.base();
                let fsg_type = base
                    .fillet_surface_generator()
                    .ok_or_else(|| missing(EaseInput::SurfaceGenerator))?
                    .fillet_surface_generator_type();
                if fsg_type == FilletSurfaceGeneratorType::BlendCurve {
                    continue; // its sections are curvature-continuous already
                }
                if fsg_type != FilletSurfaceGeneratorType::Circular {
                    return Err(stop(EaseStop::SectionsNotCircular));
                }
                // SM_CAST_PTR(SmFace, pFV->GetPointClassObject()), NER
                let Some(EntityRef::Face(face)) = class_object else {
                    return Err(missing(EaseInput::SetbackFace));
                };
                let rail_index = base.find_index_of_rail_on_face(self.target, Some(face))?;
                if rail_index > 1 {
                    return Err(stop(EaseStop::SetbackFaceCarriesNoRail));
                }
                let rail = self
                    .pseudo
                    .geom(fg)?
                    .rail(rail_index)
                    .ok_or_else(|| missing(EaseInput::RoundRail))?;
                let rail_curve = edge::curve(self.pseudo.brep(), rail)?
                    .ok_or_else(|| missing(EaseInput::EdgeCurve))?
                    .clone();
                let tol = base.this_approx_tol_3d();
                let rail_ivl = rail_curve.natural_interval();

                // w = 1 where the setback's section meets the rail
                let dropped = rail_curve.drop_point(
                    k,
                    &rail_ivl,
                    self.pseudo.vertex_point(fv)?,
                    None,
                    tol,
                    None,
                    SolverOperation::Minimize,
                )?;
                if !dropped.success {
                    return Err(stop(EaseStop::SetbackVertexOffRail));
                }
                let to = dropped.parameter;

                // w = 0 where the plane square to the edge, the ease farther from the
                // vertex, meets the rail (as SmFilletVertex::CalcSetBackVert finds the
                // setback's)
                let edgeuse = base.edgeuse(0)?.ok_or_else(|| null_defect(SOLVER_EDGES))?;
                let e = eu::edge(self.target, edgeuse)?.ok_or_else(|| null_defect(SOLVER_EDGES))?;
                // each edge's own setback (the fork's 61547af, GetSetBackDistAlong)
                let dist_at = along_edge
                    .iter()
                    .find(|(edge, _)| *edge == e)
                    .map_or(set_back_dist, |&(_, d)| d)
                    + ease_dist;
                let edge_curve = edge::curve(self.target, e)?
                    .ok_or_else(|| missing_at(EaseInput::EdgeCurve, EntityRef::Edge(e)))?;
                let edge_ivl = edge::interval(self.target, e)?;
                let from_start = edge::start_vertex(self.target, e)? == Some(cp_vertex);
                let edge_param = arc_length_param(
                    k,
                    edge_curve,
                    if from_start {
                        edge_ivl.min()
                    } else {
                        edge_ivl.max()
                    },
                    if from_start { dist_at } else { -dist_at },
                    Some(edge::tolerance(self.target, e)?),
                )?;
                if !edge_ivl.contains_value(edge_param, 0.0) {
                    return Err(stop(EaseStop::EasePastEdgeEnd));
                }
                let pv = edge_curve.evaluate(k, edge_param, 1, true, true)?;
                let plane_orig = pv.first().copied().ok_or_else(|| defect(EVALUATED))?;
                let plane_normal = pv.get(1).copied().ok_or_else(|| defect(EVALUATED))?;
                let surf1 = Surface::from(
                    base.surface(0)?
                        .map(|slot| slot.surface.surface.clone())
                        .ok_or_else(|| missing(EaseInput::SolverSurface))?,
                );
                let surf2 = Surface::from(
                    base.surface(1)?
                        .map(|slot| slot.surface.surface.clone())
                        .ok_or_else(|| missing(EaseInput::SolverSurface))?,
                );
                let domain1 = surf1.natural_uv_domain_with_nurb(k)?;
                let domain2 = surf2.natural_uv_domain_with_nurb(k)?;
                let closest = |surf: &Surface, domain: &Extent2d| -> Result<Point2> {
                    let solutions = surf.global_point_solve(
                        k,
                        Some(SurfaceHandle(surf)),
                        domain,
                        SolverOperation::Minimize,
                        plane_orig,
                        zone_tol3d(k, tol),
                        None,
                        SolutionRequested::All,
                    )?;
                    let [s] = solutions.as_slice() else {
                        return Err(stop(EaseStop::PlaneOriginNotOneClosest));
                    };
                    Ok(Point2::new(s.start.param(0)?, s.start.param(1)?))
                };
                let uv1 = closest(&surf1, &domain1)?;
                let uv2 = closest(&surf2, &domain2)?;
                let mut tsect = TsectPnt::default();
                let solver = self.solver_mut(solver_id)?;
                let found = FilletSolverOps::point_on_plane_solve(
                    live_solver_ops(solver),
                    k,
                    None,
                    plane_orig,
                    plane_normal,
                    &domain1,
                    &domain2,
                    uv1,
                    uv2,
                    &mut tsect,
                )?;
                if !found {
                    return Err(stop(EaseStop::NoSectionInPlane));
                }
                let srf = if rail_index == 0 {
                    SrfIndex::First
                } else {
                    SrfIndex::Second
                };
                let start = face2::surface(self.target, face)?
                    .ok_or_else(|| missing_at(EaseInput::FaceSurface, EntityRef::Face(face)))?
                    .evaluate_point(k, tsect.uv_pos(srf))?;
                let dropped = rail_curve.drop_point(
                    k,
                    &rail_ivl,
                    start,
                    None,
                    tol,
                    None,
                    SolverOperation::Minimize,
                )?;
                let from = dropped.parameter;
                if !dropped.success {
                    return Err(stop(EaseStop::EaseStartOffRail));
                }
                if (from - to).abs() <= EFF_ZERO {
                    return Err(stop(EaseStop::EaseHasNoLength));
                }

                // the round's ramps, a new list for a round not yet eased
                let ramp = EaseRamp { from, to };
                if let Some((_, round_ramps)) =
                    geoms.iter().zip(ramps.iter_mut()).find(|&(&g, _)| g == fg)
                {
                    round_ramps.push(ramp);
                } else {
                    geoms.push(fg);
                    ramps.push(vec![ramp]);
                }
            }
        }

        for (&fg, geom_ramps) in geoms.iter().zip(&ramps) {
            let solver_id = self
                .pseudo
                .geom(fg)?
                .solver
                .ok_or_else(|| missing(EaseInput::RoundSolver))?;
            let surface = self
                .fillet_surface(fg)?
                .ok_or_else(|| missing(EaseInput::RoundSurface))?;
            let own_surface = surface.clone();
            let own = crate::topology::brep::p3::with_nurb(k, surface)?
                .ok_or_else(|| not_bspline(EaseGeometry::RoundSurface))?
                .into_owned();
            let center = self
                .pseudo
                .geom(fg)?
                .center_line_curve()
                .ok_or_else(|| missing(EaseInput::CentreLine))?
                .clone();
            let base = self.solver(solver_id)?.base();
            let tol = base.this_approx_tol_3d();
            // each rail (0, then 1): its curve, that curve's B-spline part (for the blend's
            // construction, which changes its knots), its UV curve on the face it rolls on,
            // and that face's surface
            let rail_parts = |r: usize| -> Result<(Curve, BSplineCurve, BSplineCurve, Surface)> {
                let rail_edge = self
                    .pseudo
                    .geom(fg)?
                    .rail(r)
                    .ok_or_else(|| missing(EaseInput::RoundRail))?;
                let rail = edge::curve(self.pseudo.brep(), rail_edge)?
                    .ok_or_else(|| missing(EaseInput::EdgeCurve))?
                    .clone();
                let rail_bspline = bspline_curve(&rail)
                    .ok_or_else(|| not_bspline(EaseGeometry::RailCurve))?
                    .clone();
                // the face the rail rolls on (the solver's surface r is its offset)
                let edgeuse = base.edgeuse(r)?.ok_or_else(|| null_defect(SOLVER_EDGES))?;
                let rail_face = eu::face(self.target, edgeuse)?
                    .ok_or_else(|| missing(EaseInput::EdgeUseFace))?;
                let face_surf = face2::surface(self.target, rail_face)?
                    .ok_or_else(|| missing_at(EaseInput::FaceSurface, EntityRef::Face(rail_face)))?
                    .clone();
                // the rail on that face's surface: as the trace left it, or (an analytic
                // round, whose rails are lines) the rail dropped onto it, over the rail's
                // own parameter
                let uv = if let Some(curve) = self.pseudo.original_uv_curve(rail_edge)? {
                    bspline_curve(curve)
                        .ok_or_else(|| not_bspline(EaseGeometry::TracedUvCurve))?
                        .clone()
                } else {
                    let uv_curves = face_surf
                        .drop_curve::<NoObject>(
                            k,
                            &face_surf.natural_uv_domain_with_nurb(k)?,
                            &rail,
                            &rail.natural_interval(),
                            approx_tol3d(k, tol),
                            true,
                            None,
                            true,
                        )?
                        .uv_curves;
                    let [first] = uv_curves.as_slice() else {
                        return Err(stop(EaseStop::RailNotOneUvCurve));
                    };
                    bspline_curve(first)
                        .ok_or_else(|| not_bspline(EaseGeometry::DroppedUvCurve))?
                        .clone()
                };
                let uv_ivl = uv.natural_interval();
                let rail_ivl = rail.natural_interval();
                if (uv_ivl.min() - rail_ivl.min()).abs() > 1.0e-9 * (1.0 + rail_ivl.length())
                    || (uv_ivl.max() - rail_ivl.max()).abs() > 1.0e-9 * (1.0 + rail_ivl.length())
                {
                    return Err(stop(EaseStop::UvCurveOffRailInterval));
                }
                Ok((rail, rail_bspline, uv, face_surf))
            };
            let (rail0, mut c1, uv0, surf0) = rail_parts(0)?;
            let (_, mut c2, uv1, surf1) = rail_parts(1)?;

            // the rails' direction on the surface: the one over the rails' interval whose
            // iso-curve at a ramp's end passes through rail 0 there
            let rail_ivl = rail0.natural_interval();
            let domain = own.natural_uv_domain();
            let param_tol = 1.0e-9 * (1.0 + rail_ivl.length());
            let mut along = SurfParamType::Unknown;
            for dir in [SurfParamType::U, SurfParamType::V] {
                if along != SurfParamType::Unknown {
                    break;
                }
                let other = if dir == SurfParamType::U {
                    SurfParamType::V
                } else {
                    SurfParamType::U
                };
                let along_ivl = interval_in(&domain, dir);
                let across_ivl = interval_in(&domain, other);
                if (along_ivl.min() - rail_ivl.min()).abs() > param_tol
                    || (along_ivl.max() - rail_ivl.max()).abs() > param_tol
                {
                    continue;
                }
                let t = geom_ramps
                    .first()
                    .ok_or_else(|| {
                        defect(
                            "an eased round has no ramp: its list of ramps is made with the \
                             first ramp found for it",
                        )
                    })?
                    .to;
                let on_rail = rail0.evaluate_point(k, t)?;
                let lo = own_surface.evaluate_point(k, uv_at(dir, t, across_ivl.min()))?;
                let hi = own_surface.evaluate_point(k, uv_at(dir, t, across_ivl.max()))?;
                let dist_tol = 10.0 * tol;
                if on_rail.distance(lo) <= dist_tol || on_rail.distance(hi) <= dist_tol {
                    along = dir;
                }
            }
            if along == SurfParamType::Unknown {
                return Err(stop(EaseStop::NoDirectionAlongRails));
            }

            // the curvature-continuous surface on the same rails (CreateSurfaceFromCurves
            // changes the curves' knots: it is given copies)
            let c0 = center;
            // the fork's order-2 G^n section (WR-685, tangency-labs/smlib@11ea2d9): the
            // faces' curvature across the rails. Its thumbweight stays 0, not the equal area
            // default: it is the corner's transition, not the blend's own section (the fork's
            // 22d258a = d01d7b0's adb9854)
            let g2 = GnSectionFsg::new(2, 0.0).create_surface_from_curves(
                k,
                &c0,
                [&mut c1, &mut c2],
                tol,
                GnFaces {
                    surfaces: [&surf0, &surf1],
                    uv: [&uv0, &uv1],
                },
            )?;

            // (the fork's EaseSurface takes the G^n target, a blend by its definition, and the
            // round's own surface as they are)
            let eased = ease_surface_of(k, &own_surface, &g2, along, geom_ramps)?;
            self.pseudo
                .geom_mut(fg)?
                .set_fillet_surface(Some(Surface::from(eased)));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{SM_ERR, SM_ERR_NULL_POINTER};

    /// Each ease failure names what was missing or where the ease stopped, and keeps its
    /// status (#19825: typed causes, not phrases).
    #[test]
    fn ease_failures_name_what_failed_and_keep_their_status() {
        let missing = missing(EaseInput::CornerVertex);
        assert_eq!(
            missing.to_string(),
            "blend::fillet_executive::session::ease: invalid_input: an eased corner has no filleted vertex"
        );
        assert_eq!(missing.sm_status(), Some(SM_ERR_NULL_POINTER));
        let failed = stop(EaseStop::EasePastEdgeEnd);
        assert_eq!(
            failed.to_string(),
            "blend::fillet_executive::session::ease: outside_domain: the setback plus the ease \
             runs past the end of the filleted edge"
        );
        assert_eq!(failed.sm_status(), Some(SM_ERR));
        assert_eq!(failed.kind(), crate::error::ErrorKind::OutsideDomain);
        let cast = not_bspline(EaseGeometry::RailCurve);
        assert_eq!(cast.sm_status(), Some(SM_ERR_NULL_POINTER));
        assert_eq!(cast.kind(), crate::error::ErrorKind::Unsupported);
        let defect = null_defect(SOLVER_EDGES);
        assert!(defect.is_defect(), "{defect:?}");
        assert_eq!(defect.sm_status(), Some(SM_ERR_NULL_POINTER));
    }
}
