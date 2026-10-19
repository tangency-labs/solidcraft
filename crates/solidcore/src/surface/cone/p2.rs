//! `SmCone` part 2: the offset cone, recognition of a cone NURB, the rebuild of the
//! analytic data from an edited NURB, and the ellipse intervals inside a trimmed cone.
//!
//! Upstream: `source/SMLib/src/SmCone.cpp`, `source/SMLib/inc/SmCone.h`.
//!
//! | Rust                                         | Upstream                                         |
//! |----------------------------------------------|--------------------------------------------------|
//! | [`Cone::create_offset_surface`]              | `SmCone::CreateOffsetSurface`                    |
//! | [`Cone::is_nurb_surface_cone`]               | `SmCone::IsNurbSurfaceCone`                      |
//! | [`Cone::rebuild_step_from_nurb_parameters`]  | `SmCone::RebuildSTEPFromNURBParameters`          |
//! | [`ellipse_trim_intervals`]                   | `sm_GetEllipseTrimIntervals` (file static)       |
//! | [`Cone::reparameterize`]                     | `SmSurfOfRevolution::Reparameterize` (inherited; its polar converter rebuild calls the cone's iso curves) |
//! | [`Cone::base_mut`]                           | `GetGwNurbPointer` (non-`const`, inherited)      |
//!
//! Deviations:
//! - **Out-params** become return values: the offset is `None` where upstream succeeds
//!   without a surface (both new radii below the tolerance); the recogniser's cone is
//!   `Some` when upstream returns `TRUE`; the trim intervals are returned.
//! - **Statuses.** Calls whose status upstream ignores keep upstream's values
//!   (`SmPoint3d` outputs stay `SM_UNDEF_DOUBLE`, a failed `AngleBetween` is `0`),
//!   except that [`SolidError::Unported`] always propagates. Where upstream returns an
//!   error status through `SER` from a function returning `SmBoolean`
//!   (`IsNurbSurfaceCone`: a failed iso curve) the error is returned; upstream's caller
//!   would read that status as `TRUE` with a `NULL` cone.
//! - **Uninitialised reads.** `sm_GetEllipseTrimIntervals` ignores the status of
//!   `smgu_LinePlaneIntersect` and then reads its parameter: a seam line parallel to
//!   the ellipse plane (or of zero length) returns the error here. A failed
//!   `ConvertUVFromNURBSToSTEP` (upstream then uses a default-constructed domain) and
//!   `GetHeight` failures are returned too. A STEP inversion of the cone that fails in
//!   the generator solve returns the error (upstream keeps the location it had already
//!   classified); its other failures leave `SM_LT_EXTERIOR`, as upstream.
//! - **Offset factory.** The apex-truncation branch dereferences the result of
//!   `CreateCanonical` without checking its status (a `NULL` cone crashes, a
//!   non-finite height recurses without bound); here that construction's error is
//!   returned.
//! - **NURB.** As in `surface::cone::p1`, the cone is expected to have its NURB
//!   ([`Cone::make_nurb`]); upstream makes a missing one inside `const` queries.
//!   `Notify` (attribute copying in the recogniser) has no equivalent.
//! - **Recursion.** None of these functions recurses.
//! - **Debug code.** The `SM_DEBUG_CODE` checks and drawing are omitted.

use super::Cone;
use crate::curve::curve::Curve;
use crate::curve::ellipse::{Ellipse, EllipseOptions};
use crate::curve::line::{BoundedLineOptions, Line};
use crate::error::{ignore_status, ignored, Result, SolidError, SM_ERR};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::geom_utility::p1::{
    line_plane_intersect, line_point_distance, shell_sort_double_array,
};
use crate::geom::geom_utility::p2::{circle_plane_intersect, CurveIntersection};
use crate::geom::tol::{approx_tol3d, is_zero};
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::{max2, min2, EFF_ZERO, EFF_ZERO_SQRT, PI};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::{
    BSplineSurface, IsoCurveOptions, LocationType, SurfParamType,
};
use crate::nurbs::nurbs_crv::to_euclid;

const OP_OFFSET: &str = "surface::cone::p2::Cone::create_offset_surface";
const OP_REBUILD: &str = "surface::cone::p2::Cone::rebuild_step_from_nurb_parameters";
const OP_REPARAM: &str = "surface::cone::p2::Cone::reparameterize";
const OP_TRIM: &str = "surface::cone::p2::ellipse_trim_intervals";

/// The `SmSurfOfRevolution::STEPInversion` operation whose `SM_ERR` means "the rotated
/// point is not on the generator" (upstream sets `SM_LT_EXTERIOR` there).
const OP_REV_STEP_INV: &str = "surface::surf_of_revolution::p2::SurfOfRevolution::step_inversion";

/// `ANALYTIC_TOL_SCALE` (`SmConfig.h`).
const ANALYTIC_TOL_SCALE: f64 = 100.0;

/// A circular iso curve found by `IsArc` (or the apex of a cone with a vertex).
#[derive(Debug, Clone, Copy)]
struct IsoArc {
    /// `s*RefFrame`: origin at the centre, X towards the start point.
    frame: Axis2Placement,
    /// `d*Rad`.
    radius: f64,
    /// `d*StartAng`.
    start_ang: f64,
    /// `d*EndAng`.
    end_ang: f64,
}

/// The linear (generator) direction test shared by the recogniser and the rebuild:
/// `SM_SP_V` for a u of degree 1 with two control points, `SM_SP_U` for such a v,
/// else `SM_SP_NEITHER`.
fn linear_iso_param(s: &BSplineSurface) -> SurfParamType {
    if s.degree(SurfParamType::U) == 1 && s.number_control_points(SurfParamType::U) == 2 {
        SurfParamType::V
    } else if s.degree(SurfParamType::V) == 1 && s.number_control_points(SurfParamType::V) == 2 {
        SurfParamType::U
    } else {
        SurfParamType::Neither
    }
}

/// `pIsoCrv->IsArc(5, dScaledZero, ..)` of a plain B-spline iso curve.
fn iso_arc(curve: &BSplineCurve, scaled_zero: f64) -> Result<Option<IsoArc>> {
    let arc = crate::curve::curve::p5::is_arc_base(
        curve,
        |c, tol| c.is_degenerate(tol, None),
        5,
        scaled_zero,
    )?;
    Ok(arc.map(|(frame, radius, start_ang, end_ang)| IsoArc {
        frame,
        radius,
        start_ang,
        end_ang,
    }))
}

/// The point at the middle of the natural interval of a degenerate iso curve (status
/// ignored upstream: `SM_UNDEF_DOUBLE` on failure).
fn mid_point(curve: &BSplineCurve) -> Result<Point3> {
    Ok(ignored(curve.evaluate_point(curve.natural_interval().mid()))?.unwrap_or(Vec3::UNDEF))
}

/// The top and bottom circles of the sweep iso curves at `top_knot` and `bot_knot`
/// (with a zero-radius circle at a vertex), or `None` when they are not two arcs or an
/// arc and a degenerate curve.
fn end_circles(
    k: &Kernel,
    s: &BSplineSurface,
    sweep: SurfParamType,
    top_knot: f64,
    bot_knot: f64,
    scaled_zero: f64,
) -> Result<Option<(IsoArc, IsoArc)>> {
    // make temporary Top and Bot BSPline isoParameter curves - hopefully arc shapes
    let top_iso = s
        .create_iso_parametric_curve(k, sweep, top_knot, IsoCurveOptions::default())?
        .curve;
    let bot_iso = s
        .create_iso_parametric_curve(k, sweep, bot_knot, IsoCurveOptions::default())?
        .curve;

    // classify IsoparamCurves
    let top_degenerate = top_iso.is_degenerate(scaled_zero, None)?;
    let bot_degenerate = bot_iso.is_degenerate(scaled_zero, None)?;
    let top_arc = if top_degenerate {
        None
    } else {
        iso_arc(&top_iso, scaled_zero)?
    };
    let bot_arc = if bot_degenerate {
        None
    } else {
        iso_arc(&bot_iso, scaled_zero)?
    };

    //  2 Arcs                       = were good, continue
    //  1 Arc and a degenerate curve = cone has a vertex, set zero radius arc parameters
    //  2 degenerate curves          = not a cone, error
    match (top_arc, bot_arc) {
        (Some(top), _) if bot_degenerate => {
            // cone with a vertex
            let point = mid_point(&bot_iso)?;
            let bot = IsoArc {
                frame: Axis2Placement::new(point, top.frame.x_axis(), top.frame.y_axis()),
                radius: 0.0,
                start_ang: top.start_ang,
                end_ang: top.end_ang,
            };
            Ok(Some((top, bot)))
        }
        (_, Some(bot)) if top_degenerate => {
            // cone with a vertex
            let point = mid_point(&top_iso)?;
            let top = IsoArc {
                frame: Axis2Placement::new(point, bot.frame.x_axis(), bot.frame.y_axis()),
                radius: 0.0,
                start_ang: bot.start_ang,
                end_ang: bot.end_ang,
            };
            Ok(Some((top, bot)))
        }
        (Some(top), Some(bot)) => Ok(Some((top, bot))),
        // not a cone - Top or Bot circular isoParameter curve is degenerate or not an arc
        _ => Ok(None),
    }
}

/// Top/bot start angles, end angles or x axes vary by more than `ang_tol`.
fn twisted(top: &IsoArc, bot: &IsoArc, ang_tol: f64) -> Result<bool> {
    // (AngleBetween's status is ignored upstream: its angle is then 0)
    let twist_ang = ignored(top.frame.x_axis().angle_between(bot.frame.x_axis()))?.unwrap_or(0.0);
    Ok((top.start_ang - bot.start_ang).abs() > ang_tol
        || (top.end_ang - bot.end_ang).abs() > ang_tol
        || twist_ang.abs() > ang_tol)
}

impl Cone {
    /// The inherited NURB (`SmBSplineSurface` base object), mutably: an edit of the
    /// NURB (as through upstream's `GetGwNurbPointer`) leaves the analytic data as it
    /// was until [`Self::rebuild_step_from_nurb_parameters`].
    pub fn base_mut(&mut self) -> &mut BSplineSurface {
        &mut self.rev.base
    }

    /// Maps the NURB domain onto `trim_domain` without changing the shape (the
    /// inherited `SmSurfOfRevolution::Reparameterize`): the NURB (made first when
    /// missing, with the cone's iso curves), then the generator's NURB interval, then
    /// the polar converter rebuilt with the cone's iso curves (statuses ignored).
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` when the NURB cannot be made; those of
    /// [`crate::surface::surf_of_revolution::SurfOfRevolution::reparameterize`].
    pub fn reparameterize(&mut self, k: &Kernel, trim_domain: &Extent2d) -> Result<()> {
        // GetOrCreateGwNurbPointer (the virtual MakeNurb with the cone's iso curves)
        if self.rev.base.gw_nurb().is_none() {
            ignore_status(self.make_nurb(k))?;
        }
        if self.rev.base.gw_nurb().is_none() {
            return Err(SolidError::null_pointer(OP_REPARAM));
        }
        let inside_out = self.inside_out;
        self.rev
            .reparameterize_using(k, trim_domain, &|s, k, p, t| {
                Ok(super::p1::cone_iso_curve(s, inside_out, k, p, t, None, false)?.curve)
            })
    }

    /// Rebuilds the polar converter with the cone's iso curves
    /// (`RebuildPolarConverter`, which calls the virtual `CreateIsoParametricCurve`).
    fn rebuild_cone_polar_converter(&mut self, k: &Kernel) -> Result<()> {
        let inside_out = self.inside_out;
        self.rev.rebuild_polar_converter(k, &|s, k, p, t| {
            Ok(super::p1::cone_iso_curve(s, inside_out, k, p, t, None, false)?.curve)
        })
    }

    // ---- CreateOffsetSurface -----------------------------------------------------

    /// The cone offset by `signed_offset_distance` along the NURB normal (negated with
    /// [`crate::surface::surf_of_revolution::SurfOfRevolution::swap_uv`] and with
    /// [`Self::inside_out`]) (`CreateOffsetSurface`). The radii change by
    /// `cos(semi) d` and the origin moves by `-sin(semi) d` along the axis:
    ///
    /// * both new radii below the tolerance (`SmApproxTol3d(approx_tol_3d)`): `None`;
    /// * one of them below it: the cone truncated at its new apex, made by
    ///   [`Self::create_canonical`] (same `swap_uv` and `inside_out`), its NURB
    ///   reparameterised to the matching part of this NURB domain (upstream scales the
    ///   domain maximum, not its span), the generator's NURB interval set to match and
    ///   the polar converter rebuilt;
    /// * else a copy with the new base radius and origin, every NURB control point
    ///   moved by the offset of its generator row (computed from the next control
    ///   point at the apex) and the generator translated by the offset vector.
    ///
    /// # Errors
    ///
    /// Those of [`Self::height`] and of [`Self::create_canonical`] (upstream then
    /// dereferences a `NULL` cone); `SM_ERR_NULL_POINTER` without generator.
    pub fn create_offset_surface(
        &self,
        k: &Kernel,
        signed_offset_distance: f64,
        approx_tol_3d: f64,
    ) -> Result<Option<Self>> {
        let approx_tol_3d = approx_tol3d(k, approx_tol_3d);

        // When UVs are swapped - reverse orientation of offset
        // When InsideOut       - reverse orientation of offset
        let offset_sign = 1.0
            * (if self.rev.swap_uv { -1.0 } else { 1.0 })
            * (if self.inside_out { -1.0 } else { 1.0 });

        // Cone locals
        let origin = self.rev.position.origin();
        let x_axis = self.rev.position.x_axis();
        let y_axis = self.rev.position.y_axis();
        let z_axis = self.rev.position.z_axis();
        let height = self.height(k)?;
        let nurb_domain = self.rev.base.natural_uv_domain();

        // offset vector and displacements
        let mut sin = (self.semi_angle_deg * PI / 180.0).sin();
        let mut cos = (self.semi_angle_deg * PI / 180.0).cos();
        let norm = (cos * x_axis - sin * z_axis).unitize_or_z();
        let off_vec = offset_sign * signed_offset_distance * norm;
        let off_z_dist = -sin * offset_sign * signed_offset_distance;
        let off_rad_dist = cos * offset_sign * signed_offset_distance;

        // get new Bot and Top radii from Old and dOffRadDist
        let old_bot_radius = self.bot_radius();
        let old_top_radius = self.top_radius();
        let mut new_bot_radius = old_bot_radius + off_rad_dist;
        let mut new_top_radius = old_top_radius + off_rad_dist;

        // If both top and bottom are negative then don't create result
        if new_bot_radius < approx_tol_3d && new_top_radius < approx_tol_3d {
            return Ok(None);
        }

        // only the BotRadius or TopRadius has gone negative
        if new_bot_radius < approx_tol_3d || new_top_radius < approx_tol_3d {
            // compute BotOrigin, Height, Bot and Top radii for new truncated cone
            let (new_height, bot_origin) = if new_bot_radius < approx_tol_3d {
                let tan_ang = height / old_top_radius;
                let new_height = new_top_radius * tan_ang;
                new_bot_radius = 0.0;
                (
                    new_height,
                    origin + (height + off_z_dist - new_height) * z_axis,
                )
            } else {
                let tan_ang = height / old_bot_radius;
                let new_height = new_bot_radius * tan_ang;
                new_top_radius = 0.0;
                (new_height, origin + off_z_dist * z_axis)
            };

            // New Cone Position - truncated at new apex position
            let new_position = Axis2Placement::new(bot_origin, x_axis, y_axis);

            // Make the new cone - preserving original cone bSwapUV and bInsideOut values
            let mut off_cone = Self::create_canonical(
                k,
                &new_position,
                new_bot_radius,
                new_top_radius,
                new_height,
                self.rev.swap_uv,
                self.inside_out,
            )?;

            // compute the offsetSurf Nurb domain to be a properly trimmed subset of the
            // input Nurb domain
            let tgt_nurb_domain = if self.rev.swap_uv {
                Extent2d::from_bounds(
                    nurb_domain.min().x,
                    nurb_domain.min().y,
                    nurb_domain.min().x + new_height / height * nurb_domain.max().x,
                    nurb_domain.max().y,
                )
            } else {
                Extent2d::from_bounds(
                    nurb_domain.min().x,
                    nurb_domain.min().y,
                    nurb_domain.max().x,
                    nurb_domain.min().y + new_height / height * nurb_domain.max().y,
                )
            };

            // update the pOffCone NurbDomain (status ignored)
            ignore_status(off_cone.reparameterize(k, &tgt_nurb_domain))?;

            // adjust the GenCurve Nurb domain to be a properly trimmed subset of the
            // input NurbDomain (status ignored)
            let tgt_gen_nurb_ivl = if self.rev.swap_uv {
                tgt_nurb_domain.u_interval()
            } else {
                tgt_nurb_domain.v_interval()
            };
            let gen = off_cone
                .rev
                .gen_curve
                .as_mut()
                .ok_or(SolidError::null_pointer(OP_OFFSET))?;
            ignore_status(gen.edit_parameterization(k, &tgt_gen_nurb_ivl))?;

            // rebuild the PolarCurve to make sure all is coordinated (status ignored)
            ignore_status(off_cone.rebuild_cone_polar_converter(k))?;

            return Ok(Some(off_cone));
        }

        // copy the cone
        let mut off_cone = self.clone();

        // modify analytic parameters - new BaseRadius and new Cone Center
        off_cone.base_radius = self.base_radius + off_rad_dist;
        ignore_status(off_cone.rev.position.set_canonical(
            origin + off_z_dist * z_axis,
            x_axis,
            y_axis,
        ))?;

        // modify every Cone Nurb ControlPoint position - preserves the input NURB domain
        if let Some(off_nurb) = off_cone.rev.base.nurb.as_mut() {
            // For every m_pNurb ControlPoint - make inner iteration the GenCurve direction
            let (i_cnt, j_cnt) = if self.rev.swap_uv {
                (off_nurb.m(), off_nurb.n())
            } else {
                (off_nurb.n(), off_nurb.m())
            };
            let cp = |ii: usize, jj: usize| {
                if self.rev.swap_uv {
                    (jj, ii)
                } else {
                    (ii, jj)
                }
            };

            // for every sweep direction control point
            for ii in 0..=i_cnt {
                // get ControlPoint[i][0] Euclidean position
                let (u0, v0) = cp(ii, 0);
                let euclid_0 = to_euclid(off_nurb.cpt(u0, v0)?);

                // get the Cone radius for ControlPoint[i][0] Radius
                let p0_dot_z = (euclid_0.x - origin.x) * z_axis.x
                    + (euclid_0.y - origin.y) * z_axis.y
                    + (euclid_0.z - origin.z) * z_axis.z;
                let center = origin + p0_dot_z * z_axis;
                let vec = Vec3::new(
                    euclid_0.x - center.x,
                    euclid_0.y - center.y,
                    euclid_0.z - center.z,
                );
                let this_radius = self.radius(p0_dot_z);

                // when the radius is zero - need to compute move from other ControlPoint
                let mv = if is_zero(this_radius) {
                    // point is an apex - need to get rotation angle from next control
                    // point in genCurve direction
                    let (u1, v1) = cp(ii, 1);
                    let rotation_euclid = to_euclid(off_nurb.cpt(u1, v1)?);
                    let rotation_vec = Vec3::new(
                        rotation_euclid.x - origin.x,
                        rotation_euclid.y - origin.y,
                        rotation_euclid.z - origin.z,
                    );
                    let this_height = rotation_vec.dot(z_axis);
                    let radial_vec = rotation_vec - this_height * z_axis;
                    let radius = radial_vec.length();
                    let height_radius = self.radius(this_height);
                    // (operator/: a plain division, unlike operator/=)
                    let radial_unit = radial_vec / radius;
                    cos = radial_unit.dot(x_axis);
                    sin = radial_unit.dot(y_axis);

                    radius / height_radius * off_rad_dist * (cos * x_axis + sin * y_axis)
                        + off_z_dist * z_axis
                } else {
                    let scale = (this_radius + off_rad_dist) / this_radius;
                    vec * (scale - 1.0) + off_z_dist * z_axis
                };

                // for every genCurve directed ControlPoint
                for jj in 0..=j_cnt {
                    // apply the move to the controlPoint
                    let (u, v) = cp(ii, jj);
                    let euclid_j = to_euclid(off_nurb.cpt(u, v)?);
                    let cp_j = off_nurb.cpt_mut(u, v)?;
                    cp_j.x = (euclid_j.x + mv.x) * cp_j.w;
                    cp_j.y = (euclid_j.y + mv.y) * cp_j.w;
                    cp_j.z = (euclid_j.z + mv.z) * cp_j.w;
                }
            }
        }

        // translate the NewCone->GenCurve from current center line (status ignored)
        let mut mv = Axis2Placement::IDENTITY;
        mv.translate(off_vec);
        let gen = off_cone
            .rev
            .gen_curve
            .as_mut()
            .ok_or(SolidError::null_pointer(OP_OFFSET))?;
        ignore_status(gen.transform(k, &mv, None))?;

        Ok(Some(off_cone))
    }

    // ---- IsNurbSurfaceCone -------------------------------------------------------

    /// The cone `test_surface` is (`IsNurbSurfaceCone`), or `None`: the surface must be
    /// rational, of degree 1 with two control points in one direction and degree 2 in
    /// the other, whose knots have the multiplicities `[3, 2, .., 2, 3]`; its
    /// iso curves at both ends of the linear direction are coaxial circular arcs (or an
    /// arc and a degenerate curve: a vertex) over the same angles, and the iso curve
    /// halfway is a circle of the average radius on the same axis. Tolerances are
    /// `tolerance_scale * SM_EFF_ZERO_SQRT` for angles and
    /// `tolerance_scale * 100 * SM_EFF_ZERO * (1 + |min corner| + |max corner|)` for
    /// distances (upstream default scale `1`). The new bounded cone is reparameterised
    /// to the surface's NURB domain (its NURB is the cone's own, not a copy).
    ///
    /// # Errors
    ///
    /// Those of the iso curves (upstream returns that status as its `SmBoolean`); those
    /// of the arc test.
    pub fn is_nurb_surface_cone(
        k: &Kernel,
        test_surface: &BSplineSurface,
        tolerance_scale: f64,
    ) -> Result<Option<Self>> {
        // low work - not rational, not proper degree, not linear in one direction or other
        let deg_u = test_surface.degree(SurfParamType::U);
        let deg_v = test_surface.degree(SurfParamType::V);
        let uv_domain = test_surface.natural_uv_domain();
        let linear_iso_param = linear_iso_param(test_surface);

        // quit when surface is not properly parameterized for a potential cone
        if deg_u + deg_v != 3
            || !test_surface.is_rational()
            || linear_iso_param == SurfParamType::Neither
        {
            return Ok(None);
        }

        // check for proper knot vector in non-linear direction:
        // [tripleKnot, doubleKnot, doubleKnot, ..., tripleKnot]
        let (_, cir_mults) = test_surface.knots(linear_iso_param, None)?;
        if cir_mults.first() != Some(&3) || cir_mults.last() != Some(&3) {
            return Ok(None);
        }
        let inner = cir_mults.len().saturating_sub(2);
        if cir_mults.iter().skip(1).take(inner).any(|&m| m != 2) {
            return Ok(None);
        }

        // evaluate Nurb min/max domain points (statuses ignored)
        let min_pnt = ignored(test_surface.evaluate_point(uv_domain.min()))?.unwrap_or(Vec3::UNDEF);
        let max_pnt = ignored(test_surface.evaluate_point(uv_domain.max()))?.unwrap_or(Vec3::UNDEF);

        // scale zero value and angle tolerance
        let ang_tol = tolerance_scale * EFF_ZERO_SQRT;
        let scaled_zero = tolerance_scale
            * ANALYTIC_TOL_SCALE
            * EFF_ZERO
            * (1.0 + min_pnt.max_dimension() + max_pnt.max_dimension());

        // branch on linear direction to find circular and linear directions
        let (swap_uv, sweep_iso_param, bot_knot, top_knot) = if deg_u == 1 {
            // Need to swap UV when mapping parameters from analytic domain to nurbs domain
            (true, SurfParamType::U, uv_domain.min().x, uv_domain.max().x)
        } else {
            (
                false,
                SurfParamType::V,
                uv_domain.min().y,
                uv_domain.max().y,
            )
        };

        // classify the top and bottom isoParameter curves
        let Some((mut top, bot)) = end_circles(
            k,
            test_surface,
            sweep_iso_param,
            top_knot,
            bot_knot,
            scaled_zero,
        )?
        else {
            return Ok(None);
        };

        // check for not a cone - top/bot start angles or top/bot end angles vary
        if twisted(&top, &bot, ang_tol)? {
            return Ok(None);
        }

        // get the cone height = length of axis vector from botCurve Center to TopCurve
        // Center
        let center_top = top.frame.origin();
        let center_bot = bot.frame.origin();
        let mut nurb_z_axis = center_top - center_bot;
        let height = nurb_z_axis.length();

        // not a cone - height of cone is degenerate (cone is a disk)
        if height < EFF_ZERO_SQRT {
            return Ok(None);
        }
        nurb_z_axis = nurb_z_axis.unitize_or_z();

        // check for not a cone - bottom arc's plane is not perpendicular to cone axis
        if !nurb_z_axis.is_parallel_to(bot.frame.z_axis(), ang_tol) {
            return Ok(None);
        }

        // check for not a cone - a middle arc must also be concentric with axis, and in
        // a parallel plane
        let mid_param = (bot_knot + top_knot) / 2.0;
        let mid_iso = test_surface
            .create_iso_parametric_curve(k, sweep_iso_param, mid_param, IsoCurveOptions::default())?
            .curve;

        // It must be circular.
        let Some(mid) = iso_arc(&mid_iso, scaled_zero)? else {
            return Ok(None);
        };

        // Radius must be halfway between top and bottom.
        if ((bot.radius + top.radius) / 2.0 - mid.radius).abs() > scaled_zero {
            return Ok(None);
        }

        // check for not a cone - middle arc's plane is not perpendicular to cone axis
        if !mid.frame.z_axis().is_parallel_to(nurb_z_axis, ang_tol) {
            return Ok(None);
        }

        // check for not a cone - middle arc's center is not on cone axis
        // (smgu_LinePointDistance always succeeds)
        let (dist_to_axis, _) = line_point_distance(center_top, nurb_z_axis, mid.frame.origin());
        if dist_to_axis > scaled_zero {
            return Ok(None);
        }

        // arrive here when surface has passed all not-a-cone filters

        // By definition: the circle rotation sets the Z axis for the cone when the
        // CenterBot to CenterTop vector opposes this Z axis, bInsideOut = TRUE
        let inside_out = nurb_z_axis.dot(bot.frame.z_axis()) < 0.0;

        // For the x-axis, use the one (top or bottom) at the beginning of the param
        // range in that direction.
        let x_axis = if inside_out {
            top.frame.x_axis()
        } else {
            bot.frame.x_axis()
        };

        // get STEP YAxis from XAxis and StepZAxis
        let step_z_axis = if inside_out {
            -nurb_z_axis
        } else {
            nurb_z_axis
        };
        let y_axis = step_z_axis.cross(x_axis);

        // when top and bot radius values are nearly equal - make them exactly equal
        if (bot.radius - top.radius).abs() < EFF_ZERO * (1.0 + bot.radius) {
            top.radius = bot.radius;
        }

        // build the analytic cone surface
        // bInsideOut toggles the STEP Origin between Top and Bot NurbCircles
        let mut cone = Self::new_bounded(
            if inside_out {
                top.frame.origin()
            } else {
                bot.frame.origin()
            },
            x_axis,
            y_axis,
            if inside_out { top.radius } else { bot.radius },
            if inside_out { bot.radius } else { top.radius },
            top.start_ang,
            top.end_ang,
            height,
            swap_uv,
            inside_out,
            false,
        );

        // reparameterize the NURB so that AnalyticSurface->NurbDomain ==
        // InputSurface->NurbDomain (status ignored)
        ignore_status(cone.reparameterize(k, &test_surface.natural_uv_domain()))?;

        Ok(Some(cone))
    }

    // ---- RebuildSTEPFromNURBParameters ---------------------------------------------

    /// Rebuilds the analytic data from the NURB after an edit
    /// (`RebuildSTEPFromNURBParameters`): the circles at both ends of the linear
    /// direction (tested as [`Self::is_nurb_surface_cone`] does with the scale `1000`,
    /// the corner points evaluated on the cone, without the knot and middle circle
    /// tests) give the semi-angle (kept for equal radii), base radius, inside-out flag,
    /// placement (the end circle's frame), STEP domain and a new generator line over
    /// the NURB's linear interval; the polar converter is rebuilt before the generator
    /// is replaced.
    ///
    /// # Errors
    ///
    /// `SM_ERR` when the NURB is no longer a cone; those of the iso curves.
    pub fn rebuild_step_from_nurb_parameters(&mut self, k: &Kernel) -> Result<()> {
        // check m_pNurb - a degree 2 rational surface in at least 1 direction
        let deg_u = self.rev.base.degree(SurfParamType::U);
        let deg_v = self.rev.base.degree(SurfParamType::V);
        let nurb_uv_domain = self.rev.base.natural_uv_domain();
        let linear_iso_param = linear_iso_param(&self.rev.base);
        let linear_ivl = match linear_iso_param {
            SurfParamType::V => nurb_uv_domain.u_interval(),
            SurfParamType::U => nurb_uv_domain.v_interval(),
            _ => Extent1d::new(),
        };

        // no work - modified parameterization
        if deg_u + deg_v != 3
            || !self.rev.base.is_rational()
            || linear_iso_param == SurfParamType::Neither
        {
            // Modified Nurb SmCone: no longer has degree 1 and 2 basis functions
            return Err(SolidError::sm_err(OP_REBUILD));
        }

        // evaluate Nurb min/max domain points (the cone's EvaluatePoint; statuses
        // ignored)
        let nurb_min_point =
            ignored(self.evaluate_point(k, nurb_uv_domain.min()))?.unwrap_or(Vec3::UNDEF);
        let nurb_max_point =
            ignored(self.evaluate_point(k, nurb_uv_domain.max()))?.unwrap_or(Vec3::UNDEF);

        // scaled zero value (loose because surf is already labeled analytic)
        let tolerance_scale = 1000.0;
        let ang_tol = tolerance_scale * EFF_ZERO_SQRT;
        let scaled_zero = tolerance_scale
            * ANALYTIC_TOL_SCALE
            * EFF_ZERO
            * (1.0 + nurb_min_point.max_dimension() + nurb_max_point.max_dimension());

        // branch on linear direction to find circular and linear directions
        let (swap_uv, sweep_iso_param, bot_knot, top_knot) = if deg_u == 1 {
            (
                true,
                SurfParamType::U,
                nurb_uv_domain.min().x,
                nurb_uv_domain.max().x,
            )
        } else {
            (
                false,
                SurfParamType::V,
                nurb_uv_domain.min().y,
                nurb_uv_domain.max().y,
            )
        };

        // classify the top and bottom isoParameter curves (SmBSplineSurface's)
        let Some((mut top, bot)) = end_circles(
            k,
            &self.rev.base,
            sweep_iso_param,
            top_knot,
            bot_knot,
            scaled_zero,
        )?
        else {
            // Modified Nurb SmCone: no longer has circular top and bottom IsoParamCurves
            return Err(SolidError::sm_err(OP_REBUILD));
        };

        // not a cone - top/bot start angles or top/bot end angles vary by more than
        // tolerance
        if twisted(&top, &bot, ang_tol)? {
            return Err(SolidError::sm_err(OP_REBUILD));
        }

        // get the cone height = length of axis vector from botCurve Center to TopCurve
        // Center
        let center_top = top.frame.origin();
        let center_bot = bot.frame.origin();
        let mut nurb_z_axis = center_top - center_bot;
        let height = nurb_z_axis.length();

        // not a cone - height of cone is degenerate (cone is a disk)
        if height < EFF_ZERO_SQRT {
            return Err(SolidError::sm_err(OP_REBUILD));
        }
        nurb_z_axis = nurb_z_axis.unitize_or_z();

        // not a cone - bottom arc's plane is not perpendicular to cone axis
        if !nurb_z_axis.is_parallel_to(bot.frame.z_axis(), ang_tol) {
            return Err(SolidError::sm_err(OP_REBUILD));
        }

        // when the CenterBot to CenterTop vector opposes the circle Z axis, bInsideOut
        let inside_out = nurb_z_axis.dot(bot.frame.z_axis()) < 0.0;

        // (upstream also computes the STEP Y axis from the top frame's X axis here;
        // it is unused)

        // when top and bot radius values are nearly equal - make them exactly equal
        if (bot.radius - top.radius).abs() < EFF_ZERO * (1.0 + bot.radius) {
            top.radius = bot.radius;
        }

        // modify m_dSemiAngleDeg for nonCylinder shapes
        let radius_diff = if inside_out {
            bot.radius - top.radius
        } else {
            top.radius - bot.radius
        };
        if radius_diff.abs() > EFF_ZERO {
            self.semi_angle_deg = radius_diff.atan2(height) * 180.0 / PI;
        }

        // set Analytic parameters
        self.base_radius = if inside_out { top.radius } else { bot.radius };
        self.inside_out = inside_out;
        self.rev.planar_generator = true;
        self.rev.position = if inside_out { top.frame } else { bot.frame };
        self.rev.swap_uv = swap_uv;
        let _ = self
            .rev
            .anal_uv_domain
            .set_min_max_coords(top.start_ang, 0.0, top.end_ang, height);

        // Make the generator line
        let line_point = self.rev.position.origin() + self.base_radius * self.rev.position.x_axis();
        let line_vec = height * self.rev.position.z_axis()
            + (self.radius(height) - self.base_radius) * self.rev.position.x_axis();
        let length = line_vec.length();
        let mut line = Line::bounded(
            line_point,
            line_vec,
            &Extent1d::from_min_max(0.0, height),
            &BoundedLineOptions {
                scale: length / height,
                dim: 3,
                nurb: None,
                inside_out,
            },
        );

        // set the NurbDomain (status ignored)
        ignore_status(line.edit_parameterization(&linear_ivl))?;

        // Build newCone PolarConverter from Nurb rotation isoParamCurve (status
        // ignored), then save the gencurve
        ignore_status(self.rebuild_cone_polar_converter(k))?;
        self.rev.gen_curve = Some(Curve::from(line));
        Ok(())
    }
}

// ---- sm_GetEllipseTrimIntervals ----------------------------------------------------

/// The location the cone's `STEPInversion` writes for `point` (its status is ignored
/// upstream).
fn cone_location(k: &Kernel, cone: &Cone, point: Point3, tol: f64) -> Result<LocationType> {
    match cone.step_inversion(k, point, tol, None) {
        Ok(inv) => Ok(inv.location),
        Err(e) if e.is_unported() => Err(e),
        // rotated point not on the generator: upstream sets SM_LT_EXTERIOR
        Err(SolidError::Upstream {
            operation: OP_REV_STEP_INV,
            code: SM_ERR,
            ..
        }) => Ok(LocationType::Exterior),
        Err(e) => {
            // a failed TransformPointToStartPlane returns after SM_LT_EXTERIOR is set
            let pos = cone.positive_point(k, point)?;
            if cone
                .rev
                .transform_point_to_start_plane(pos.point, tol, false)
                .is_err()
            {
                Ok(LocationType::Exterior)
            } else {
                Err(e)
            }
        }
    }
}

/// The segment classification of `sm_GetEllipseTrimIntervals` (`bInside[ii]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Inside {
    /// `FALSE`.
    No,
    /// `TRUE`.
    Yes,
    /// `UNSURE`: a degenerate segment, classified by its neighbours.
    Unsure,
}

/// The intervals (in degrees of the ellipse's STEP domain) of the ellipse
/// `origin + x_radius cos(t) x_axis + y_radius sin(t) y_axis`, known to lie on the
/// cone, that are inside the cone trimmed to its NURB domain `uv_domain`
/// (`sm_GetEllipseTrimIntervals`). The full ellipse is intersected with the top and
/// bottom circles (two-point crossings inside the sweep only) and the two seam lines
/// (within the scaled zero of their ends, or always when a circle lies in the ellipse
/// plane); the crossing angles, sorted, cut it into segments whose midpoints are
/// classified by the cone's STEP inversion (a degenerate segment is inside when both
/// neighbours are outside). Every inside segment is intersected periodically with
/// `[start_ang_deg, end_ang_deg]`. Fewer than two crossings give no interval.
///
/// # Errors
///
/// Those of [`Ellipse::new`] and [`Cone::height`]; a seam line parallel to the
/// ellipse plane or of zero length, and a failed NURB to STEP conversion of the domain
/// corners (upstream then reads uninitialised values); a STEP inversion failing in the
/// generator solve.
pub fn ellipse_trim_intervals(
    k: &Kernel,
    ellipse_origin: Point3,
    ellipse_x_axis: Vec3,
    ellipse_y_axis: Vec3,
    ellipse_x_radius: f64,
    ellipse_y_radius: f64,
    ellipse_start_ang_deg: f64,
    ellipse_end_ang_deg: f64,
    cone: &Cone,
    uv_domain: &Extent2d,
    tol: f64,
) -> Result<Vec<Extent1d>> {
    // init output
    let mut ivls = Vec::new();

    // find ellipse/coneTrimBoundary intersections (at most 6)
    let mut xsect_angle: Vec<f64> = Vec::with_capacity(6);

    // ellipse locals
    let ellipse_z = ellipse_x_axis.cross(ellipse_y_axis);
    let ellipse_ivl = Extent1d::from_min_max(ellipse_start_ang_deg, ellipse_end_ang_deg);
    let full_ellipse = Ellipse::new(
        k,
        ellipse_origin,
        ellipse_x_axis,
        ellipse_y_axis,
        Extent1d::from_min_max(0.0, 360.0),
        ellipse_x_radius,
        ellipse_y_radius,
        &EllipseOptions {
            dim: 3,
            ..EllipseOptions::default()
        },
    )?;

    // cone locals
    let height = cone.height(k)?;
    let cone_axis = cone.rev.position.z_axis();
    let center_bot = cone.rev.position.origin() + cone.rev.anal_uv_domain.min().y * cone_axis;
    let center_top = center_bot + height * cone_axis;

    // map given NurbDomain into StepDomain (SmSurface::ConvertDomainFromNURBSToSTEP:
    // a reversed result is replaced by the NURB domain)
    let min_step_uv = cone.convert_uv_from_nurbs_to_step(k, uv_domain.min())?;
    let max_step_uv = cone.convert_uv_from_nurbs_to_step(k, uv_domain.max())?;
    let mut anal_uv_domain = Extent2d::new();
    if anal_uv_domain
        .set_min_max_coords(
            min_step_uv.x,
            min2(min_step_uv.y, max_step_uv.y),
            max_step_uv.x,
            max2(min_step_uv.y, max_step_uv.y),
        )
        .is_err()
    {
        anal_uv_domain = *uv_domain;
    }

    // get cone corners [StartLine start, StartLine end, EndLine start EndLine end]
    // (statuses ignored)
    let corner = |u: f64, v: f64| -> Result<Point3> {
        Ok(
            ignored(cone.evaluate_step_point(anal_uv_domain.evaluate(u, v)))?
                .unwrap_or(Vec3::UNDEF),
        )
    };
    let corners = [
        corner(0.0, 0.0)?,
        corner(0.0, 1.0)?,
        corner(1.0, 0.0)?,
        corner(1.0, 1.0)?,
    ];
    let [corner0, corner1, corner2, corner3] = corners;
    let start_line_vec = corner1 - corner0;
    let end_line_vec = corner3 - corner2;

    // get ellipse/TopCircle(max 2), ellipse/BotCircle(max 2) ellipse/StartSeamLine(max
    // 1), ellipse/EndSeamLine(max 1) intersections (the circle statuses are ignored:
    // their count is 0 then)
    let circle_hits = |radius: f64, center: Point3| -> Result<CurveIntersection> {
        Ok(ignored(circle_plane_intersect(
            tol,
            radius,
            center,
            cone_axis,
            ellipse_origin,
            ellipse_z,
        ))?
        .unwrap_or(CurveIntersection::Points(Vec::new())))
    };
    let mut tsect1 = circle_hits(cone.top_radius(), center_top)?;
    let mut tsect2 = circle_hits(cone.bot_radius(), center_bot)?;
    let param0 = line_plane_intersect(corner0, start_line_vec, ellipse_origin, ellipse_z)?;
    let param1 = line_plane_intersect(corner2, end_line_vec, ellipse_origin, ellipse_z)?;

    // Check for degenerate solutions, within tol. [B10]
    for tsect in [&mut tsect1, &mut tsect2] {
        if let CurveIntersection::Points(points) = tsect {
            if let [a, b] = points.as_slice() {
                if a.distance(*b) < tol {
                    points.truncate(1);
                }
            }
        }
    }
    let coincident = matches!(tsect1, CurveIntersection::Coincident)
        || matches!(tsect2, CurveIntersection::Coincident);

    // Consider all 4 possible ConeCircle/Ellipse intersections: only two-point
    // intersections, only points in the cone trim boundary (not on its seams)
    for tsect in [&tsect1, &tsect2] {
        let CurveIntersection::Points(points) = tsect else {
            continue;
        };
        if points.len() != 2 {
            continue;
        }
        for &point in points {
            // map XSectPoint to ConeUV point
            let loc = cone_location(k, cone, point, tol)?;
            if loc == LocationType::Interior && loc != LocationType::USeam {
                // add ellipse angleDeg for intersection to in/out angle array
                xsect_angle.push(full_ellipse.step_inversion(k, point, false, 0.0).0);
            }
        }
    }

    // for StartSeamLine/ellipse and EndSeamLine/ellipse xSects
    let scaled_zero = EFF_ZERO * (1.0 + max2(corner0.max_dimension(), corner3.max_dimension()));
    for (line_start, line_vec, param) in [
        (corner0, start_line_vec, param0),
        (corner2, end_line_vec, param1),
    ] {
        if coincident || (param >= 0.0 - scaled_zero && param <= 1.0 + scaled_zero) {
            let point = line_start + param * line_vec;
            xsect_angle.push(full_ellipse.step_inversion(k, point, false, 0.0).0);
        }
    }

    // When there are no intersections the whole curve must lie outside the cone
    // trim domain.
    let cnt = xsect_angle.len();
    if cnt < 2 {
        return Ok(ivls);
    }

    // order the XSectPoints by angle
    shell_sort_double_array(&mut xsect_angle);

    // the start and end angles of segment ii (the last one includes the zero angle:
    // its start is moved down by 360)
    let segment = |ii: usize| -> Result<(f64, f64)> {
        let next = (ii + 1) % cnt;
        let this_ang = *xsect_angle
            .get(ii)
            .ok_or(SolidError::invalid_input(OP_TRIM, "segment index"))?;
        let next_ang = *xsect_angle
            .get(next)
            .ok_or(SolidError::invalid_input(OP_TRIM, "segment index"))?;
        let start = if next == 0 {
            this_ang - 360.0
        } else {
            this_ang
        };
        Ok((start, next_ang))
    };

    // now classify the segment midPoints as in/out
    let scaled_ang_zero = EFF_ZERO * 100.0 * 360.0;
    let mut inside = Vec::with_capacity(cnt);
    for ii in 0..cnt {
        let (start_ang_deg, end_ang_deg) = segment(ii)?;

        // mark degenerate intervals as unknown for now
        if end_ang_deg - start_ang_deg < scaled_ang_zero {
            inside.push(Inside::Unsure);
            continue;
        }

        // classify nonDegenerate interval midPoint
        let mid_ang_deg = (end_ang_deg + start_ang_deg) / 2.0;
        let mid_point = full_ellipse.evaluate_step_point(mid_ang_deg)?;
        let loc = cone_location(k, cone, mid_point, tol)?;
        inside.push(if loc == LocationType::Exterior {
            Inside::No
        } else {
            Inside::Yes
        });
    }

    // add one interval to output for every inside segment
    for ii in 0..cnt {
        let next = (ii + 1) % cnt;
        let prev = (ii + cnt - 1) % cnt;
        let state = |i: usize| inside.get(i).copied();
        let (start_ang_deg, end_ang_deg) = segment(ii)?;

        // when the segment is inside
        if state(ii) == Some(Inside::Yes)
            || (state(ii) == Some(Inside::Unsure)
                && state(prev) == Some(Inside::No)
                && state(next) == Some(Inside::No))
        {
            // make and trim interval to ellipse interval (AddValue: start can be above
            // end by machine noise)
            let mut untrimmed_ivl = Extent1d::new();
            untrimmed_ivl.add_value(start_ang_deg);
            untrimmed_ivl.add_value(end_ang_deg);
            ivls.extend(ellipse_ivl.intersect_periodic(&untrimmed_ivl, 360.0, false)?);
        }
    }
    Ok(ivls)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::axis2_placement::Axis2Placement;

    fn kernel() -> Kernel {
        Kernel::default()
    }

    fn canonical(k: &Kernel, bot: f64, top: f64, h: f64, swap: bool, inside: bool) -> Cone {
        Cone::create_canonical(k, &Axis2Placement::IDENTITY, bot, top, h, swap, inside).unwrap()
    }

    #[test]
    fn recognises_its_own_nurb() {
        let k = kernel();
        for (swap, inside) in [(false, false), (true, false), (false, true), (true, true)] {
            let cone = canonical(&k, 2.0, 1.0, 3.0, swap, inside);
            let found = Cone::is_nurb_surface_cone(&k, cone.rev().base(), 1.0)
                .unwrap()
                .unwrap();
            assert!((found.bot_radius() - cone.bot_radius()).abs() < 1e-9);
            assert!((found.top_radius() - cone.top_radius()).abs() < 1e-9);
            assert_eq!(found.rev().swap_uv(), swap);
        }
    }

    #[test]
    fn rejects_non_cones() {
        let k = kernel();
        let bs = BSplineSurface::create_bilinear_surface(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
        )
        .unwrap();
        assert!(Cone::is_nurb_surface_cone(&k, &bs, 1.0).unwrap().is_none());
    }

    #[test]
    fn rebuild_keeps_the_cone() {
        let k = kernel();
        let mut cone = canonical(&k, 2.0, 1.0, 3.0, false, false);
        let before = cone.clone();
        cone.rebuild_step_from_nurb_parameters(&k).unwrap();
        assert!((cone.base_radius() - before.base_radius()).abs() < 1e-9);
        assert!((cone.semi_angle_deg() - before.semi_angle_deg()).abs() < 1e-7);
    }

    #[test]
    fn offset_moves_radii() {
        let k = kernel();
        let cone = canonical(&k, 2.0, 1.0, 3.0, false, false);
        let off = cone.create_offset_surface(&k, 0.5, 1e-6).unwrap().unwrap();
        let semi = cone.semi_angle_deg() * PI / 180.0;
        assert!((off.base_radius() - (2.0 + semi.cos() * 0.5)).abs() < 1e-12);
        // both radii below the tolerance: no surface
        assert!(cone
            .create_offset_surface(&k, -10.0, 1e-6)
            .unwrap()
            .is_none());
        // one radius below the tolerance: truncated at the new apex
        let trunc = cone.create_offset_surface(&k, -1.2, 1e-6).unwrap().unwrap();
        assert!(trunc.top_radius().abs() < 1e-12);
    }

    #[test]
    fn offset_with_non_finite_truncation_height_returns() {
        // bottom radius 0 and a negative top radius: the truncation height is h / 0
        let k = kernel();
        let mut cone = Cone::new_bounded(
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            0.0,
            -1.0,
            0.0,
            360.0,
            2.0,
            false,
            false,
            false,
        );
        cone.make_nurb(&k).unwrap();
        for d in [0.5, 1e-3, 3.0, -0.5] {
            let _ = cone.create_offset_surface(&k, d, 1e-6);
        }
    }

    #[test]
    fn trim_of_a_cross_section() {
        let k = kernel();
        let cone = canonical(&k, 2.0, 1.0, 3.0, false, false);
        // the circle at half height, a little tilted plane through the axis point
        let r = cone.radius(1.5);
        let ivls = ellipse_trim_intervals(
            &k,
            Vec3::new(0.0, 0.0, 1.5),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            r,
            r,
            0.0,
            360.0,
            &cone,
            &cone.rev().base().natural_uv_domain(),
            1e-6,
        );
        // the circle lies in no seam-line plane crossing: the result is whatever the
        // classification gives, but it must not fail
        assert!(ivls.is_ok());
    }
}
