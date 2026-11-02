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
//! `GetOrCreateGwNurbPointer`); a kind the cast refuses is `SM_ERR_NULL_POINTER` (`NER`). The
//! orientation test of `sm_EaseSurface` evaluates that B-spline part where the fork calls the
//! copy's (analytic) `EvaluatePoint`; the rail drop onto a face's surface runs without the face's
//! surface cache. Every `SER(SM_ERR)` is `SM_ERR`; out-params are return values. No recursion.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // the fork's NaN-safe tests

use super::{live_solver_ops, SessionObjects};
use crate::blend::blend_setback::{ease_surface_of, interval_in, uv_at, EaseRamp};
use crate::blend::fillet_geom::p1::{FilletGeomId, FilletVertexType, SurfaceHandle};
use crate::blend::fillet_section::section::{GnFaces, GnSectionFsg};
use crate::blend::fillet_solver::p1::{
    FilletSolverOps, FilletSolverType, FilletSurfaceGeneratorType,
};
use crate::blend::fillet_standard_solver::p1::{BlendCurveCrossSectionFsg, RailSurface};
use crate::curve::curve::{Curve, CurveKind};
use crate::error::{EntityRef, Result, SolidError};
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
use crate::topology::{edge, VertexId};

use super::FilletObject;

const OP: &str = "blend::fillet_executive::session::ease";

/// `SM_ERR`.
fn sm_err() -> SolidError {
    SolidError::sm_err(OP)
}

/// `SM_ERR_NULL_POINTER` (`NER`).
fn null() -> SolidError {
    SolidError::null_pointer(OP)
}

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
        Err(SolidError::Upstream {
            operation: OP,
            code: r.status(),
            entity: None,
        })
    }
}

impl SessionObjects<'_> {
    /// `SmFilletExecutive::EaseSetBackSections` (the fork's, tangency-labs/smlib `e74bbd3`):
    /// once the fillet surfaces exist and before the corners' edges are computed, for each
    /// `SM_FV_SETBACK` vertex of a corner with an ease (in `corners`' order), the round's
    /// surface is replaced, over the rail parameter from where the plane square to the edge
    /// at the setback plus the ease cuts the rail to where the setback's section meets it,
    /// by the C2 blend of its own surface into the one `SmBlendCurveCrossSectionFSG` (`G2`,
    /// at thirds) builds on the same rails. A round whose sections are already that
    /// (`SM_FSG_BLEND_CURVE`) keeps them; ramps on both ends of one round go into one
    /// blend. Without an eased corner nothing changes.
    ///
    /// # Errors
    /// `SM_ERR` and `SM_ERR_NULL_POINTER` where the fork `SER`s and `NER`s; those of the
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
            let dist_at = corner.set_back_dist() + corner.ease_dist();
            let cp_vertex: VertexId = corner.filleted_vertex().ok_or_else(null)?;
            let mut fvs = Vec::new();
            corner.fillet_vertices(&mut fvs);
            for fv in fvs {
                let vertex = self.pseudo.fillet_vertex(fv)?;
                if vertex.fillet_vertex_type() != FilletVertexType::Setback {
                    continue;
                }
                if !vertex.is_processed() {
                    return Err(sm_err());
                }
                let class_object = vertex.point_class_object();
                let fv_geoms = self.pseudo.vertex_fillet_geoms(fv)?;
                if fv_geoms.len() != 1 {
                    return Err(sm_err());
                }
                let fg = *fv_geoms.first().ok_or_else(sm_err)?;
                let solver_id = self.pseudo.geom(fg)?.solver.ok_or_else(null)?;
                let base = self.solver(solver_id)?.base();
                let fsg_type = base
                    .fillet_surface_generator()
                    .ok_or_else(null)?
                    .fillet_surface_generator_type();
                if fsg_type == FilletSurfaceGeneratorType::BlendCurve {
                    continue; // its sections are curvature-continuous already
                }
                if fsg_type != FilletSurfaceGeneratorType::Circular {
                    return Err(sm_err());
                }
                // SM_CAST_PTR(SmFace, pFV->GetPointClassObject()), NER
                let Some(EntityRef::Face(face)) = class_object else {
                    return Err(null());
                };
                let rail_index = base.find_index_of_rail_on_face(self.target, Some(face))?;
                if rail_index > 1 {
                    return Err(sm_err());
                }
                let rail = self.pseudo.geom(fg)?.rail(rail_index).ok_or_else(null)?;
                let rail_curve = edge::curve(self.pseudo.brep(), rail)?
                    .ok_or_else(null)?
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
                    return Err(sm_err());
                }
                let to = dropped.parameter;

                // w = 0 where the plane square to the edge, the ease farther from the
                // vertex, meets the rail (as SmFilletVertex::CalcSetBackVert finds the
                // setback's)
                let edgeuse = base.edgeuse(0)?.ok_or_else(null)?;
                let e = eu::edge(self.target, edgeuse)?.ok_or_else(null)?;
                let edge_curve = edge::curve(self.target, e)?.ok_or_else(null)?;
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
                    return Err(sm_err());
                }
                let pv = edge_curve.evaluate(k, edge_param, 1, true, true)?;
                let plane_orig = pv.first().copied().ok_or_else(sm_err)?;
                let plane_normal = pv.get(1).copied().ok_or_else(sm_err)?;
                let surf1 = Surface::from(
                    base.surface(0)?
                        .map(|slot| slot.surface.surface.clone())
                        .ok_or_else(null)?,
                );
                let surf2 = Surface::from(
                    base.surface(1)?
                        .map(|slot| slot.surface.surface.clone())
                        .ok_or_else(null)?,
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
                    if solutions.len() != 1 {
                        return Err(sm_err());
                    }
                    let s = solutions.get(0).ok_or_else(sm_err)?;
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
                    return Err(sm_err());
                }
                let srf = if rail_index == 0 {
                    SrfIndex::First
                } else {
                    SrfIndex::Second
                };
                let start = face2::surface(self.target, face)?
                    .ok_or_else(null)?
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
                if !dropped.success || (from - to).abs() <= EFF_ZERO {
                    return Err(sm_err());
                }

                let index = if let Some(index) = geoms.iter().position(|&g| g == fg) {
                    index
                } else {
                    geoms.push(fg);
                    ramps.push(Vec::new());
                    geoms.len() - 1
                };
                ramps
                    .get_mut(index)
                    .ok_or_else(sm_err)?
                    .push(EaseRamp { from, to });
            }
        }

        for (&fg, geom_ramps) in geoms.iter().zip(&ramps) {
            let solver_id = self.pseudo.geom(fg)?.solver.ok_or_else(null)?;
            let surface = self.fillet_surface(fg)?.ok_or_else(null)?;
            let own_surface = surface.clone();
            let own = crate::topology::brep::p3::with_nurb(k, surface)?
                .ok_or_else(null)?
                .into_owned();
            let center = self
                .pseudo
                .geom(fg)?
                .center_line_curve()
                .ok_or_else(null)?
                .clone();
            let base = self.solver(solver_id)?.base();
            let tol = base.this_approx_tol_3d();
            let mut rails: Vec<Curve> = Vec::new();
            let mut uvs: Vec<BSplineCurve> = Vec::new();
            let mut face_surfs: Vec<Surface> = Vec::new();
            for r in 0..2 {
                let rail_edge = self.pseudo.geom(fg)?.rail(r).ok_or_else(null)?;
                let rail = edge::curve(self.pseudo.brep(), rail_edge)?
                    .ok_or_else(null)?
                    .clone();
                if bspline_curve(&rail).is_none() {
                    return Err(null());
                }
                // the face the rail rolls on (the solver's surface r is its offset)
                let edgeuse = base.edgeuse(r)?.ok_or_else(null)?;
                let rail_face = eu::face(self.target, edgeuse)?.ok_or_else(null)?;
                let face_surf = face2::surface(self.target, rail_face)?
                    .ok_or_else(null)?
                    .clone();
                // the rail on that face's surface: as the trace left it, or (an analytic
                // round, whose rails are lines) the rail dropped onto it, over the rail's
                // own parameter
                let uv = if let Some(curve) = self.pseudo.original_uv_curve(rail_edge)? {
                    bspline_curve(curve).ok_or_else(null)?.clone()
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
                    if uv_curves.len() != 1 {
                        return Err(sm_err());
                    }
                    let first = uv_curves.first().ok_or_else(sm_err)?;
                    bspline_curve(first).ok_or_else(null)?.clone()
                };
                let uv_ivl = uv.natural_interval();
                let rail_ivl = rail.natural_interval();
                if (uv_ivl.min() - rail_ivl.min()).abs() > 1.0e-9 * (1.0 + rail_ivl.length())
                    || (uv_ivl.max() - rail_ivl.max()).abs() > 1.0e-9 * (1.0 + rail_ivl.length())
                {
                    return Err(sm_err());
                }
                rails.push(rail);
                uvs.push(uv);
                face_surfs.push(face_surf);
            }
            let (Some(rail0), Some(rail1)) = (rails.first(), rails.get(1)) else {
                return Err(sm_err());
            };

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
                let t = geom_ramps.first().ok_or_else(sm_err)?.to;
                let on_rail = rail0.evaluate_point(k, t)?;
                let lo = own_surface.evaluate_point(k, uv_at(dir, t, across_ivl.min()))?;
                let hi = own_surface.evaluate_point(k, uv_at(dir, t, across_ivl.max()))?;
                let dist_tol = 10.0 * tol;
                if on_rail.distance(lo) <= dist_tol || on_rail.distance(hi) <= dist_tol {
                    along = dir;
                }
            }
            if along == SurfParamType::Unknown {
                return Err(sm_err());
            }

            // the curvature-continuous surface on the same rails (CreateSurfaceFromCurves
            // changes the curves' knots: it is given copies)
            let mut c0 = center;
            let mut c1 = bspline_curve(rail0).ok_or_else(null)?.clone();
            let mut c2 = bspline_curve(rail1).ok_or_else(null)?.clone();
            let g2_generator = BlendCurveCrossSectionFsg {
                continuity: 2,
                blend_scale: 1.0,
                at_thirds: true,
            };
            let solver = self.solver_mut(solver_id)?;
            let need_g1 = live_solver_ops(solver).solver_type()? == FilletSolverType::ConstRadius;
            let (Some(surf0), Some(surf1)) = (face_surfs.first(), face_surfs.get(1)) else {
                return Err(sm_err());
            };
            let g2 = if k.fixes.fillet_gn_section_generator {
                // the fork's order-2 G^n section (WR-685, tangency-labs/smlib@11ea2d9):
                // the faces' curvature across the rails. Its thumbweight stays 0, not the
                // equal area default: it is the corner's transition, not the blend's own
                // section (the fork's 22d258a = d01d7b0's adb9854)
                let (Some(uv0), Some(uv1)) = (uvs.first(), uvs.get(1)) else {
                    return Err(null());
                };
                GnSectionFsg::new(2, 0.0).create_surface_from_curves(
                    k,
                    &c0,
                    [&mut c1, &mut c2],
                    tol,
                    GnFaces {
                        surfaces: [surf0, surf1],
                        uv: [uv0, uv1],
                    },
                )?
            } else {
                g2_generator
                    .create_surface_from_curves(
                        k,
                        &mut c0,
                        &mut c1,
                        &mut c2,
                        tol,
                        need_g1,
                        RailSurface {
                            surface: Some(surf0),
                            uv_curve: uvs.first(),
                        },
                        RailSurface {
                            surface: Some(surf1),
                            uv_curve: uvs.get(1),
                        },
                    )
                    .map(Surface::from)?
            };

            // (the fork's EaseSurface takes the G^n target, a blend by its definition with
            // `fillet_round_as_definition`, and the round's own surface as they are)
            let eased = ease_surface_of(k, &own_surface, &g2, along, geom_ramps)?;
            self.pseudo
                .geom_mut(fg)?
                .set_fillet_surface(Some(Surface::from(eased)));
        }
        Ok(())
    }
}
