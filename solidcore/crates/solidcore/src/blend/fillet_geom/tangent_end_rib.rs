//! The end rib of a fillet at a tangent end, solved from the tangent end outward.
//!
//! Upstream: the SolidCraft fork's `source/SMLib/src/SmFilletGeom.cpp`
//! (tangency-labs/smlib@856b54e; the zero-speed rib in `t` of an odd order past 1,
//! tangency-labs/smlib@cd47efd; WR-600).
//!
//! | Rust                         | Upstream                    |
//! |------------------------------|-----------------------------|
//! | [`section_contact_order`]    | `sm_SectionContactOrder`    |
//! | `tangent_end_rib_point` (private) | `sm_TangentEndRibPoint` |
//! | [`tangent_end_rib`]          | `sm_TangentEndRib`          |
//!
//! Deviations: out-params become return values (`None` for `rbFound == FALSE`); the
//! context argument is dropped. No recursion: the station count doubles from 8 to 1024
//! in a loop (a zero-speed rib's grows by the spans it halves, to at most 1024), each
//! Newton solve takes at most 40 steps.
//!
//! Failures (#15034): no check here decides a refusal. A contact order that cannot be
//! taken, and a rib that cannot be solved, fitted or certified within tolerance (an end
//! off a surface or its rails, a singular tangent end, a station without its point, a
//! rib that does not reach its other end, a stop of #10876 that does not fit) are each
//! a [`BlendFailure::StageFailed`], a defect, built where it happens (its origin is that
//! file and line), keeping upstream's status for the goldens: `SM_ERR_NULL_POINTER` for
//! the missing surface generator (a `NER`), `SM_ERR` otherwise. The caller,
//! `calc_fillet_int_side_face`, returns an order's failure at once and takes a rib's
//! down its failure branch, as upstream.

mod certificate;

use crate::blend::fillet_section::certificate::NurbsCurve;
use crate::blend::fillet_section::section::GnSectionFsg;
use crate::blend::fillet_solver::p1::{FilletSolver, FilletSurfaceGeneratorType};
use crate::curve::curve::Curve;
use crate::error::blend::BlendFailure;
use crate::error::{Failure, Result, SolidError, SM_ERR_NULL_POINTER};
use crate::geom::global_solver::SolverOperation;
use crate::geom::krawczyk::{inverse_of_columns3, newton, Linearised};
use crate::geom::vector2d::Point2;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve::p2::ControlPointForm;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::crv_fit::p1::ParamMethod;
use crate::nurbs::knots::Side;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::Surface;

const OP_ORDER: &str = "blend::fillet_geom::tangent_end_rib::section_contact_order";
const OP_RIB: &str = "blend::fillet_geom::tangent_end_rib::tangent_end_rib";

/// `smos_Max`.
fn smax(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// `sm_SectionContactOrder`: the order of contact `k` of a fillet's cross-section with
/// the faces it meets: 1 for a chamfer's straight section, 2 for a round's circular
/// one; a G^n section's is [`gn_section_contact_order`]'s. The section stands off its face as
/// `c s^k` at distance `s` from the contact line; where the face runs tangent into an
/// end face whose normal curvature across the seam differs by `dk`, the end rib leaves
/// the tangent end as `a ~ s^(k/2)` along the rail.
///
/// # Errors
///
/// `SM_ERR_NULL_POINTER` without surface generator; `SM_ERR` for an unknown one.
pub fn section_contact_order(solver: &FilletSolver) -> Result<u32> {
    let fsg = solver
        .fillet_surface_generator()
        .ok_or_else(|| missing(OP_ORDER))?;
    match fsg.fillet_surface_generator_type() {
        FilletSurfaceGeneratorType::Linear => Ok(1),
        FilletSurfaceGeneratorType::Circular => Ok(2),
        // ((SmBlendCurveCrossSectionFSG*)pFSG)->GetContinuity() + 1: that generator is
        // retired (WR-685); a G^n section's order is [`gn_section_contact_order`]'s
        FilletSurfaceGeneratorType::BlendCurve | FilletSurfaceGeneratorType::Unknown => {
            Err(stage_failed(OP_ORDER))
        }
    }
}

/// The fork's `sm_SectionContactOrder` for a G^n section (`SM_FSG_BLEND_CURVE`, WR-685;
/// tangency-labs/smlib@11ea2d9): the order of
/// contact k of the section with the face it meets at `at`, a point of one of its rails:
/// `n + 1`, or `n + 2` where its jets make the `t^(n+1)` term vanish (two planes), found at
/// that rail's station (`SmGnSectionFSG::ContactOrderAt`). The rail through `at` is the one
/// whose drop of `at` (over its natural interval, within `tol`) is nearest.
///
/// # Errors
///
/// `SM_ERR` when `at` is on no rail within `tol`; those of
/// the drops, the evaluations and [`GnSectionFsg::contact_order_at`].
#[allow(clippy::too_many_arguments)]
pub fn gn_section_contact_order(
    k: &Kernel,
    g: &GnSectionFsg,
    faces: [&Surface; 2],
    center: &BSplineCurve,
    rails: [&Curve; 2],
    uvs: [&Curve; 2],
    tol: f64,
    at: Point3,
) -> Result<u32> {
    // the rail through crAt, and its parameter there
    let mut rail = 2usize;
    let mut s = 0.0;
    let mut best = BIG_DOUBLE;
    for (r, c) in rails.iter().enumerate() {
        let d = c.drop_point(
            k,
            &c.natural_interval(),
            at,
            None,
            tol,
            None,
            SolverOperation::Minimize,
        )?;
        if d.success && d.distance < best {
            best = d.distance;
            rail = r;
            s = d.parameter;
        }
    }
    if rail > 1 || best > tol {
        // "sm_SectionContactOrder(): the point is on no rail"
        return Err(stage_failed(OP_ORDER));
    }
    let [uv0, uv1] = uvs;
    let [r0, r1] = rails;
    let w0 = uv0.evaluate_point(k, s)?;
    let w1 = uv1.evaluate_point(k, s)?;
    let seed = [w0.x, w0.y, w1.x, w1.y];
    let points = [r0.evaluate_point(k, s)?, r1.evaluate_point(k, s)?];
    let order = g.contact_order_at(k, faces, center, s, seed, points, rail)?;
    u32::try_from(order).map_err(|_| stage_failed(OP_ORDER))
}

/// `D[0][0]`, `D[1][0]` (u) and `D[0][1]` (v) of `surface` at `uv`
/// (`Evaluate(uv, 1, 1, TRUE, TRUE, TRUE, .., FALSE)`).
fn first_derivs(k: &Kernel, surface: &Surface, uv: Point2) -> Result<(Vec3, Vec3, Vec3)> {
    let flags = EvaluateFlags {
        u_side: Side::Left,
        v_side: Side::Left,
        only_upper_half: true,
        non_zero_tangents: false,
    };
    let d = surface.evaluate(k, uv, 1, 1, flags)?;
    let at = |i: usize, j: usize| {
        d.get(i)
            .and_then(|row| row.get(j))
            .copied()
            .ok_or_else(|| stage_failed(OP_RIB))
    };
    Ok((at(0, 0)?, at(1, 0)?, at(0, 1)?))
}

/// `sm_TangentEndRibPoint`: the end rib's point at one fillet section, the point of the
/// fillet surface at section parameter `sigma` on the end surface, by Newton on
/// (`alpha`, the end surface's uv) from the guesses. `section` is the fillet surface's
/// section parameter (0 = u, 1 = v), `alpha` the other one. Returns the solution
/// `(alpha, end uv, point)`, or `None` when Newton does not converge in 40 steps or the
/// system is singular.
fn tangent_end_rib_point(
    k: &Kernel,
    fillet: &Surface,
    end: &Surface,
    section: usize,
    sigma: f64,
    alpha: f64,
    end_uv: Point2,
) -> Result<Option<(f64, Point2, Point3)>> {
    let mut alpha = alpha;
    let mut end_uv = end_uv;
    for iter in 0..40 {
        let fil_uv = if section == 0 {
            Point2::new(sigma, alpha)
        } else {
            Point2::new(alpha, sigma)
        };
        let (f, f_u, f_v) = first_derivs(k, fillet, fil_uv)?;
        let (e, e_u, e_v) = first_derivs(k, end, end_uv)?;
        let r = f - e;
        let a = if section == 0 { f_v } else { f_u }; // d/d alpha
        let eu = -e_u;
        let ev = -e_v;
        let scale = smax(1.0, f.max_dimension());
        if r.length() <= 4.0 * EFF_ZERO * scale && iter > 0 {
            return Ok(Some((alpha, end_uv, e)));
        }
        let det = a.dot(eu.cross(ev));
        if det.abs() <= EFF_ZERO * a.length() * eu.length() * ev.length() {
            return Ok(None);
        }
        let minus_r = -r;
        let d_a = minus_r.dot(eu.cross(ev)) / det;
        let d_eu = a.dot(minus_r.cross(ev)) / det;
        let d_ev = a.dot(eu.cross(minus_r)) / det;
        alpha += d_a;
        end_uv.x += d_eu;
        end_uv.y += d_ev;
    }
    Ok(None)
}

/// Not upstream (#17744): the rib point `(alpha, end uv, x)` of [`tangent_end_rib_point`]
/// at `sigma`, its Newton steps continued until they settle (`geom::krawczyk::newton`), or
/// the point as it is where they do not move it or fail. The residual stop leaves alpha off
/// by the residual over the Jacobian's least singular value, which vanishes toward the
/// tangent end: on a G3 round's rib 4e-5 along the rail, which the certificate sees at the
/// stations. Only a rib on enclosed faces takes it; one on a face not enclosed keeps
/// upstream's point.
///
/// # Errors
///
/// A cancellation.
fn settled_point(
    k: &Kernel,
    fillet: &Surface,
    end: &Surface,
    section: usize,
    sigma: f64,
    (a, e, x): (f64, Point2, Point3),
) -> Result<(f64, Point2, Point3)> {
    let settle = |y: [f64; 3]| -> Result<Linearised<3>> {
        let [a, u, v] = y;
        let fil_uv = if section == 0 {
            Point2::new(sigma, a)
        } else {
            Point2::new(a, sigma)
        };
        let (Ok((f, f_u, f_v)), Ok((x, e_u, e_v))) = (
            first_derivs(k, fillet, fil_uv),
            first_derivs(k, end, Point2::new(u, v)),
        ) else {
            return Ok(None);
        };
        let f_a = if section == 0 { f_v } else { f_u };
        let r = f - x;
        let col = |c: Vec3| [c.x, c.y, c.z];
        Ok(Some(([r.x, r.y, r.z], [col(f_a), col(-e_u), col(-e_v)])))
    };
    let start = [a, e.x, e.y];
    Ok(match newton(start, &settle, &inverse_of_columns3)? {
        Some([a1, u, v]) if [a1, u, v] != start => {
            let e1 = Point2::new(u, v);
            let x1 = first_derivs(k, end, e1).ok().map(|(x1, _, _)| x1);
            x1.map_or((a, e, x), |x1| (a1, e1, x1))
        }
        _ => (a, e, x),
    })
}

/// The curves of [`tangent_end_rib`].
#[derive(Debug, Clone)]
pub struct TangentEndRib {
    /// `rp3DCurve`.
    pub curve_3d: BSplineCurve,
    /// `rpFilletUV`: its UV curve on the fillet surface.
    pub fillet_uv: BSplineCurve,
    /// `rpEndUV`: its UV curve on the end surface.
    pub end_uv: BSplineCurve,
    /// Not upstream (#17744): the largest distance, certified span by span, of each curve
    /// from the rib of the surfaces as the kernel evaluates them, at the same parameter (the
    /// 3D curve's point, a UV curve's point on its surface), at most the tolerance. A blend
    /// inside a scope that answers by its net is that net; one evaluated by its definition
    /// is the definition. `None` where the rib makes no claim:
    ///
    /// - a face is not enclosed (a STEP, offset, curve-bounded or in-volume face, a cone or
    ///   cylinder, a face in power-basis form, or a definition of contact order 2): the rib
    ///   is built as upstream builds it, on upstream's points and its spans' midpoints;
    /// - the faces are enclosed, but near the tangent end, `[0, t*]`, the rib is not bounded
    ///   within the tolerance (`t*` where the certificate's graded pieces stop): every other
    ///   part of the rib is certified within the tolerance as for `Some`, the first span is
    ///   compared at its midpoint as upstream, and `[0, t*]` carries no claim.
    pub bound: Option<f64>,
}

/// `sm_TangentEndRib`: the end rib of a fillet at a tangent end, the fillet surface's
/// intersection with the end surface from the tangent end `tangent_end` (on one rail,
/// where the fillet surface touches the end surface) to `other_end` (on the other rail).
///
/// The rib is solved section by section from the tangent end outward, each point exact
/// (`sm_TangentEndRibPoint`), and never marched into the tangent end, where the two
/// surfaces touch. With `s` the section parameter from the tangent end's rail and `a`
/// the along parameter, `a ~ s^(k/2)` there (`order` is `k`,
/// [`section_contact_order`]); the rib is taken in `t` with `s ~ t^2` for odd `k` and
/// `s ~ t` for even `k`, in which `a` is analytic. The three curves (3D, on the fillet
/// surface, on the end surface) interpolate the same exact points in `t`, the stations
/// doubled from 8 to 1024 until each span is certified within `approx_tol` (not upstream,
/// #17744: upstream compares the span's midpoint with the exact point there; the certificate
/// bounds every parameter of the span, `certificate`, and the largest bound is
/// [`TangentEndRib::bound`]; where a face is not enclosed, the span's midpoint is compared
/// as upstream; where only the tangent end's piece `[0, t*]` is not bounded, the first span
/// is compared at its midpoint as upstream, the rest certified, and the rib has no bound).
/// For odd `k` past 1 (the fork's tangency-labs/smlib@cd47efd) the
/// curves have zero speed at the tangent end: their first derivative there is zero
/// (the first two control points coincide) and their second the exact
/// `2 (s1 - s0) dX/ds`, `X` the fillet surface along its section, both interpolated as
/// end conditions; only the spans not certified are halved, from 8 uniform ones. With `reverse` the rib runs from `other_end` to `tangent_end`.
///
/// Not upstream (#10876): with `stop`, the rib is the piece from the tangent end to `stop`, a
/// point of the fillet surface and the end surface between the rails (where the rib passes
/// onto the next cliff face across a tangent seam, `FilletCorner::seam_cap`). `other_end`, on
/// the other rail, then only orients the section (it need not lie on the end surface), and
/// the piece is solved and fitted from the tangent end to `stop` as the whole rib is to
/// `other_end`; with `reverse` it runs from `stop` to the tangent end.
///
/// # Errors
///
/// `SM_ERR` when an end is off either surface, the ends are not on the fillet surface's
/// two rails or lie on one section, the rib does not reach its other end, a station
/// has no exact point, or (where a face is not enclosed) no station count gives the rib's
/// midpoints within tolerance; [`BlendFailure::NotCertified`] (#17744)
/// where both faces are enclosed and no station count certifies the rib within tolerance;
/// for a zero-speed rib, when the end surface is
/// singular at the tangent end, the fillet surface's section leaves its tangent plane
/// there, or an interpolation's second control point leaves its end; a stage failure
/// ([`BlendFailure::StageFailed`], a defect) for a `stop` off either
/// surface or not strictly between the rails on the other rail's side; those of the drops,
/// evaluations and fits.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // one upstream routine
pub fn tangent_end_rib(
    k: &Kernel,
    fillet: &Surface,
    end: &Surface,
    order: u32,
    tangent_end: Point3,
    other_end: Point3,
    stop: Option<Point3>,
    reverse: bool,
    approx_tol: f64,
) -> Result<TangentEndRib> {
    // both ends on the fillet surface and on the end surface
    let fil_dom = fillet.natural_uv_domain_with_nurb(k)?;
    let end_dom = end.natural_uv_domain_with_nurb(k)?;
    let opts = DropPointOptions::default();
    let mut fil_uv = [Point2::new(0.0, 0.0); 2];
    let mut end_uv = [Point2::new(0.0, 0.0); 2];
    for (kk, p) in [tangent_end, other_end].into_iter().enumerate() {
        let d =
            fillet.drop_point::<crate::surface::surface::p4::NoObject>(k, p, &fil_dom, &opts)?;
        if !d.success || d.gap > approx_tol {
            // an end of the rib is off the fillet surface
            return Err(stage_failed(OP_RIB));
        }
        // with a stop, the other rail's end orients the section only: it is off the end
        // surface where the rib passes onto the next face before reaching it
        if kk == 1 && stop.is_some() {
            if let Some(f) = fil_uv.get_mut(kk) {
                *f = d.result_uv;
            }
            continue;
        }
        let e = end.drop_point::<crate::surface::surface::p4::NoObject>(k, p, &end_dom, &opts)?;
        if !e.success || e.gap > approx_tol {
            // an end of the rib is off the end surface
            return Err(stage_failed(OP_RIB));
        }
        if let (Some(f), Some(g)) = (fil_uv.get_mut(kk), end_uv.get_mut(kk)) {
            *f = d.result_uv;
            *g = e.result_uv;
        }
    }
    let [fil0, fil1] = fil_uv;
    let [end0, end1] = end_uv;

    // the section parameter: the rails bound the fillet surface across it, one end on each
    let u_ivl = fil_dom.u_interval();
    let v_ivl = fil_dom.v_interval();
    let tol_u = EFF_ZERO_SQRT * smax(1.0, u_ivl.length());
    let tol_v = EFF_ZERO_SQRT * smax(1.0, v_ivl.length());
    let across = |a: f64, b: f64, ivl: crate::geom::extent1d::Extent1d, tol: f64| {
        ((a - ivl.min()).abs() <= tol && (b - ivl.max()).abs() <= tol)
            || ((a - ivl.max()).abs() <= tol && (b - ivl.min()).abs() <= tol)
    };
    let section = if across(fil0.x, fil1.x, u_ivl, tol_u) {
        0
    } else if across(fil0.y, fil1.y, v_ivl, tol_v) {
        1
    } else {
        // the rib's ends are not on the fillet surface's two rails
        return Err(stage_failed(OP_RIB));
    };
    let (sigma0, mut sigma1, alpha0, mut alpha1) = if section == 0 {
        (fil0.x, fil1.x, fil0.y, fil1.y)
    } else {
        (fil0.y, fil1.y, fil0.x, fil1.x)
    };
    // Not upstream (#10876): the piece to `stop`, between the rails on the other rail's side
    let mut end1 = end1;
    let mut last_point = other_end;
    if let Some(stop) = stop {
        let d =
            fillet.drop_point::<crate::surface::surface::p4::NoObject>(k, stop, &fil_dom, &opts)?;
        if !d.success || d.gap > approx_tol {
            return Err(stage_failed(OP_RIB)); // the rib's stop is off the fillet surface
        }
        let e =
            end.drop_point::<crate::surface::surface::p4::NoObject>(k, stop, &end_dom, &opts)?;
        if !e.success || e.gap > approx_tol {
            return Err(stage_failed(OP_RIB)); // the rib's stop is off the end surface
        }
        let (sigma_stop, alpha_stop) = if section == 0 {
            (d.result_uv.x, d.result_uv.y)
        } else {
            (d.result_uv.y, d.result_uv.x)
        };
        let tol_sigma = if section == 0 { tol_u } else { tol_v };
        // strictly inside, towards the other rail (a NaN fails)
        let between = (sigma_stop - sigma0) * (sigma1 - sigma0) > 0.0
            && (sigma_stop - sigma0).abs() > tol_sigma
            && (sigma1 - sigma_stop).abs() > tol_sigma
            && (sigma1 - sigma_stop) * (sigma1 - sigma0) > 0.0;
        if !between {
            return Err(stage_failed(OP_RIB)); // the rib's stop is not between the rails
        }
        sigma1 = sigma_stop;
        alpha1 = alpha_stop;
        end1 = e.result_uv;
        last_point = stop;
    }
    if (alpha1 - alpha0).abs() <= EFF_ZERO_SQRT * smax(1.0, alpha0.abs()) {
        // the rib's ends lie on one section
        return Err(stage_failed(OP_RIB));
    }
    let odd = order % 2 == 1;
    let sigma_at = |t: f64| sigma0 + (sigma1 - sigma0) * (if odd { t * t } else { t });
    // The curves' parameter is t. For an odd order past 1 the rib leaves the tangent end
    // with zero speed in t (a ~ t^k with s ~ t^2): the curves' end conditions there, the
    // first derivative zero and the second 2 (s1 - s0) times the fillet surface's
    // derivative along its section (on the fillet surface's UV: that section direction;
    // on the end surface's: the uv derivative that maps to it, in its tangent plane)
    // (the fork's tangency-labs/smlib@cd47efd).
    let zero_speed = odd && order > 1;
    let (d2_x, d2_fil, d2_end) = if zero_speed {
        let (_, f_u, f_v) = first_derivs(k, fillet, fil0)?;
        let (_, e_u, e_v) = first_derivs(k, end, end0)?;
        let two_d_sigma = 2.0 * (sigma1 - sigma0);
        let d2_x = (if section == 0 { f_u } else { f_v }) * two_d_sigma;
        let d2_fil = if section == 0 {
            Vec3::new(two_d_sigma, 0.0, 0.0)
        } else {
            Vec3::new(0.0, two_d_sigma, 0.0)
        };
        // E_u u'' + E_v v'' = X'': the normal equations of the end surface's tangent plane
        let a = e_u.dot(e_u);
        let b = e_u.dot(e_v);
        let c = e_v.dot(e_v);
        let det = a * c - b * b;
        if det <= EFF_ZERO * a * c {
            // the end surface is singular at the tangent end
            return Err(stage_failed(OP_RIB));
        }
        let r_u = e_u.dot(d2_x);
        let r_v = e_v.dot(d2_x);
        let d2_end = Vec3::new((c * r_u - b * r_v) / det, (a * r_v - b * r_u) / det, 0.0);
        // X'' lies in the end surface's tangent plane: the fillet surface touches it there
        let off_plane = d2_x - e_u * d2_end.x - e_v * d2_end.y;
        if off_plane.length() > EFF_ZERO_SQRT * smax(1.0, d2_x.length()) {
            // the fillet surface's section leaves the end surface's tangent plane at the
            // tangent end
            return Err(stage_failed(OP_RIB));
        }
        (d2_x, d2_fil, d2_end)
    } else {
        (Vec3::ZERO, Vec3::ZERO, Vec3::ZERO)
    };
    // One curve through the points at their stations `ts`: in the zero-speed case with
    // those end conditions.
    let interpolate = |pts: &[Point3], ts: &[f64], d2: Vec3| -> Result<BSplineCurve> {
        if zero_speed {
            let mut c = BSplineCurve::create_interpolating_curve_with_params(
                ParamMethod::Uniform,
                3,
                3,
                pts,
                &[Vec3::ZERO],
                Some(&[d2]),
                false,
                Some(ts),
            )?;
            // The zero first derivative is the first two control points' coincidence:
            // the interpolation's solve gives the second to rounding, and it is the
            // first.
            if c.number_control_points()? < 2 {
                return Err(stage_failed(OP_RIB));
            }
            let (cpt0, _) = c.control_point(ControlPointForm::NonRational, 0)?;
            let (cpt1, _) = c.control_point(ControlPointForm::NonRational, 1)?;
            if cpt1.distance(cpt0) > EFF_ZERO_SQRT * smax(1.0, cpt0.max_dimension()) {
                // the zero-speed end's interpolation leaves its end
                return Err(stage_failed(OP_RIB));
            }
            c.set_control_point(ControlPointForm::NonRational, 1, cpt0, 1.0)?;
            return Ok(c);
        }
        BSplineCurve::interpolate_points(pts, Some(ts), 3, None, None, false, ParamMethod::Uniform)
    };

    // the faces as the certificate encloses them (#17744): the surfaces the kernel
    // evaluates; where one is not enclosed, the rib is built as upstream builds it, its
    // spans' midpoints against the exact points there, with no bound
    let faces = match (
        certificate::Face::of(k, fillet)?,
        certificate::Face::of(k, end)?,
    ) {
        (Some(f), Some(e)) => Some((f, e)),
        _ => None,
    };
    let certified = faces.is_some();

    // the exact rib point at t, from a guess; on the other end's side of the tangent end;
    // settled where both faces are enclosed (#17744, [`settled_point`]), upstream's point
    // where a face is not
    let solve = |t: f64, alpha: f64, uv: Point2| -> Result<Option<(f64, Point2, Point3)>> {
        let sigma = sigma_at(t);
        let Some(point) = tangent_end_rib_point(k, fillet, end, section, sigma, alpha, uv)? else {
            return Ok(None);
        };
        let found = if certified {
            settled_point(k, fillet, end, section, sigma, point)?
        } else {
            point
        };
        // (`<= 0.0` fails the point; a NaN product keeps it, as upstream's test)
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        Ok(Some(found).filter(|&(a, _, _)| !((a - alpha0) * (alpha1 - alpha0) <= 0.0)))
    };
    let rib_newton = |t: f64, y: [f64; 3]| -> Result<Option<[f64; 3]>> {
        let [a, u, v] = y;
        Ok(solve(t, a, Point2::new(u, v))?.map(|(a, e, _)| [a, e.x, e.y]))
    };

    // The stations, and the exact points there. A regular rib's stations are
    // t = j / spans, the spans doubled until the curves are within the tolerance. A
    // zero-speed rib's are refined where they are not: only the spans not certified
    // (#17744; upstream: whose midpoint is off) are halved, at their exact midpoint, so the stations follow the rib's own
    // smoothness in t, the tangent end costing none.
    let mut ts: Vec<f64> = Vec::new();
    let mut alphas: Vec<f64> = Vec::new();
    let mut es: Vec<Point2> = Vec::new();
    let mut xs: Vec<Point3> = Vec::new();
    let max_spans: usize = 1024;
    let mut spans: usize = 8;
    let mut stations = false;
    let mut halvings = 0;
    // a zero-speed rib's midpoint without an exact point ends the refinement (upstream's
    // break), with upstream's failure, not as not certified
    let mut unsolved = false;
    while spans <= max_spans {
        if !stations {
            // stations t = j / spans, solved from the tangent end outward
            ts.clear();
            alphas.clear();
            es.clear();
            xs.clear();
            ts.push(0.0);
            alphas.push(alpha0);
            es.push(end0);
            xs.push(tangent_end);
            let mut all = true;
            for jj in 1..=spans {
                #[allow(clippy::cast_precision_loss)] // at most 1024
                let t = jj as f64 / spans as f64;
                // guess: the chord to the other end first, then the last two points
                // carried on
                let (mut alpha, mut uv) = if jj == 1 {
                    (
                        alpha0 + (alpha1 - alpha0) * t,
                        Point2::new(
                            end0.x + (end1.x - end0.x) * t,
                            end0.y + (end1.y - end0.y) * t,
                        ),
                    )
                } else {
                    let (Some(&a1), Some(&a2), Some(&e1), Some(&e2)) = (
                        alphas.get(jj - 1),
                        alphas.get(jj - 2),
                        es.get(jj - 1),
                        es.get(jj - 2),
                    ) else {
                        return Err(stage_failed(OP_RIB));
                    };
                    (
                        2.0 * a1 - a2,
                        Point2::new(2.0 * e1.x - e2.x, 2.0 * e1.y - e2.y),
                    )
                };
                if jj == spans {
                    alpha = alpha1;
                    uv = end1;
                }
                let Some((a, e, x)) = solve(t, alpha, uv)? else {
                    all = false;
                    break;
                };
                ts.push(t);
                alphas.push(a);
                es.push(e);
                xs.push(x);
            }
            if !all {
                spans *= 2;
                continue;
            }
            let last = xs.get_mut(spans).ok_or_else(|| stage_failed(OP_RIB))?;
            if last.distance(last_point) > approx_tol {
                // the rib does not reach its other end
                return Err(stage_failed(OP_RIB));
            }
            *last = last_point;
            stations = true;
        }

        // the three curves through the exact points
        let mut fil_pts = Vec::new();
        let mut end_pts = Vec::new();
        for ((&t, &a), e) in ts.iter().zip(&alphas).zip(&es) {
            let sig = sigma_at(t);
            fil_pts.push(if section == 0 {
                Point3::new(sig, a, 0.0)
            } else {
                Point3::new(a, sig, 0.0)
            });
            end_pts.push(Point3::new(e.x, e.y, 0.0));
        }
        let mut c3d = interpolate(&xs, &ts, d2_x)?;
        let mut c_fil = interpolate(&fil_pts, &ts, d2_fil)?;
        let mut c_end = interpolate(&end_pts, &ts, d2_end)?;

        // each span's certified bound (#17744); the spans not bounded within the tolerance,
        // with the exact rib point at their midpoint
        let rib = match &faces {
            Some((fillet, end)) => Some(certificate::Rib {
                kernel: k,
                fillet,
                end,
                section,
                sigma0,
                d_sigma: sigma1 - sigma0,
                odd,
                order,
                curves: [
                    NurbsCurve::of(&c3d)?,
                    NurbsCurve::of(&c_fil)?,
                    NurbsCurve::of(&c_end)?,
                ],
            }),
            None => None,
        };
        let mut solved = true;
        let mut off: Vec<(usize, f64, Point2, Point3)> = Vec::new();
        let mut bound = 0.0_f64;
        // the tangent end's span certified down to t* only: [0, t*] carries no claim
        let mut open = false;
        for jj in 0..spans {
            if !(zero_speed || off.is_empty()) {
                break;
            }
            let (Some(&t0), Some(&t1), Some(&a0), Some(&a1), Some(&e0), Some(&e1)) = (
                ts.get(jj),
                ts.get(jj + 1),
                alphas.get(jj),
                alphas.get(jj + 1),
                es.get(jj),
                es.get(jj + 1),
            ) else {
                return Err(stage_failed(OP_RIB));
            };
            let t = 0.5 * (t0 + t1);
            let alpha = 0.5 * (a0 + a1);
            let uv = Point2::new(0.5 * (e0.x + e1.x), 0.5 * (e0.y + e1.y));
            let mut upstream_check = rib.is_none();
            if let Some(rib) = &rib {
                let ends = ((t0, [a0, e0.x, e0.y]), (t1, [a1, e1.x, e1.y]));
                let within = if jj == 0 {
                    // the span at the tangent end, on pieces graded toward it
                    rib.first_span_bound(&rib_newton, ends.0 .1, ends.1, approx_tol)?
                } else {
                    rib.span_bound(&rib_newton, ends.0, ends.1, approx_tol)?
                };
                match within {
                    certificate::Bound::Within(d) if d <= approx_tol => {
                        bound = bound.max(d);
                        continue;
                    }
                    // certified down to t* below t1, [0, t*] not bounded: the span is
                    // compared at its midpoint as upstream compares it, and the rib makes
                    // no claim
                    certificate::Bound::Open => {
                        open = true;
                        upstream_check = true;
                    }
                    _ => {}
                }
            }
            let Some((alpha, uv, exact)) = solve(t, alpha, uv)? else {
                solved = false;
                break;
            };
            // a face not enclosed, or the tangent end's span open: upstream's check, the
            // span's midpoint against the exact rib point there
            if upstream_check {
                let on_3d = c3d.evaluate_point(t)?;
                let on_fil_uv = c_fil.evaluate_point(t)?;
                let on_end_uv = c_end.evaluate_point(t)?;
                let on_fil = fillet.evaluate_point(k, Point2::new(on_fil_uv.x, on_fil_uv.y))?;
                let on_end = end.evaluate_point(k, Point2::new(on_end_uv.x, on_end_uv.y))?;
                let off_mid = on_3d.distance(exact) > approx_tol
                    || on_fil.distance(exact) > approx_tol
                    || on_end.distance(exact) > approx_tol;
                if !off_mid {
                    continue;
                }
            }
            off.push((jj, alpha, uv, exact));
        }

        if solved && off.is_empty() {
            c_fil.convert_to_2d()?;
            c_end.convert_to_2d()?;
            if reverse {
                let ivl = c3d.natural_interval();
                c3d.reverse_parameterization(&ivl)?;
                c_fil.reverse_parameterization(&ivl)?;
                c_end.reverse_parameterization(&ivl)?;
            }
            return Ok(TangentEndRib {
                curve_3d: c3d,
                fillet_uv: c_fil,
                end_uv: c_end,
                bound: (rib.is_some() && !open).then_some(bound),
            });
        }

        if !zero_speed {
            // (only a fit the loop attempts counts: none past `max_spans`)
            if spans < max_spans {
                halvings += spans;
            }
            spans *= 2;
            stations = false;
            continue;
        }

        // the zero-speed rib: each span off the rib halved at its exact midpoint
        if !solved {
            unsolved = true;
            break;
        }
        for &(jj, alpha, uv, exact) in off.iter().rev() {
            let at = jj + 1;
            let (Some(&t0), Some(&t1)) = (ts.get(at - 1), ts.get(at)) else {
                return Err(stage_failed(OP_RIB));
            };
            ts.insert(at, 0.5 * (t0 + t1));
            alphas.insert(at, alpha);
            es.insert(at, uv);
            xs.insert(at, exact);
        }
        halvings += off.len();
        spans += off.len();
    }

    // no station count certifies the rib within the tolerance (#17744); upstream's check:
    // no stations give the rib within tolerance
    if faces.is_some() && !unsolved {
        return Err(SolidError::blend(
            OP_RIB,
            BlendFailure::NotCertified {
                halvings,
                stations: ts.len(),
            },
        ));
    }
    // no station count gives the rib within the tolerance
    Err(stage_failed(OP_RIB))
}

/// A step whose object is not there (upstream's `NER`): a defect keeping
/// `SM_ERR_NULL_POINTER` (#15034).
#[track_caller]
fn missing(op: &'static str) -> SolidError {
    Failure::new(op, BlendFailure::StageFailed)
        .with_status(SM_ERR_NULL_POINTER)
        .into_error()
}

/// A step that could not go on, with nothing finer to say (#15034 rule 2).
#[track_caller]
fn stage_failed(op: &'static str) -> SolidError {
    SolidError::blend(op, BlendFailure::StageFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::blend::BlendFailure;
    use crate::error::SM_ERR_NULL_POINTER;
    use crate::nurbs::b_spline_curve::KnotType;
    use crate::nurbs::b_spline_surface::{
        BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData,
    };

    /// A solver without a surface generator has no contact order: a stage failure, a
    /// defect, that keeps the bare `Upstream`'s status (#13790, #15034).
    #[test]
    fn a_solver_without_surface_generator_has_no_contact_order() {
        let solver = FilletSolver::new(1e-5, 1e-3, 0.017);
        let e = section_contact_order(&solver).unwrap_err();
        assert_eq!(e.operation(), OP_ORDER);
        assert_eq!(
            (e.blend_failure(), e.upstream_status()),
            (Some(&BlendFailure::StageFailed), Some(SM_ERR_NULL_POINTER))
        );
    }

    /// The fillet surface `S(u, v) = (2v − 1, u, 0)` (its section along u, its rails at
    /// u = 0 and 1) and an end surface `E(u, v) = (u − 1/2 + b(u), u, 2v − 1)` that holds the
    /// rib `x = y − 1/2 + b(y)`: `b` is one cubic B-spline of height `bump` on
    /// `[0.38, 0.42]`, between the station t = 3/8 and its span's midpoint 7/16, zero at
    /// every station and midpoint of 8 spans.
    fn bumped_rib_surfaces(bump: f64) -> (Surface, Surface) {
        let p = Point3::new;
        let fillet = BSplineSurface::create_bilinear_surface(
            p(-1.0, 0.0, 0.0),
            p(-1.0, 1.0, 0.0),
            p(1.0, 0.0, 0.0),
            p(1.0, 1.0, 0.0),
        )
        .unwrap();
        let knots = [0.0, 0.25, 0.38, 0.39, 0.4, 0.41, 0.42, 0.5, 0.75, 1.0];
        let mult = [4, 1, 1, 1, 1, 1, 1, 1, 1, 4];
        let full: Vec<f64> = knots
            .iter()
            .zip(mult)
            .flat_map(|(&x, m)| std::iter::repeat_n(x, m))
            .collect();
        let mut pts = Vec::new();
        for i in 0..full.len() - 4 {
            // the Greville abscissa: y = u exactly, and x = y − 1/2 off the bump
            let y = (full[i + 1] + full[i + 2] + full[i + 3]) / 3.0;
            let x = y - 0.5 + if i == 5 { bump } else { 0.0 };
            pts.push(p(x, y, -1.0));
            pts.push(p(x, y, 1.0));
        }
        let end = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 3,
            v_degree: 1,
            control_points: &pts,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &mult,
            v_multiplicities: &[2, 2],
            u_knots: &knots,
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: None,
            uv_domain: None,
        })
        .unwrap();
        (Surface::from(fillet), Surface::from(end))
    }

    /// A rib that passes every span's midpoint but leaves the end surface between the
    /// stations (#17744): the bump of 1e-3 on the end surface lies between a station and
    /// the next midpoint of 8 spans. On `main` it is returned 4.7e-4 off the end surface;
    /// the certificate refines it until it follows the bump, its three curves within its
    /// bound and the bound within the tolerance.
    #[test]
    fn a_rib_off_the_end_surface_between_its_stations_is_refined() {
        let k = Kernel::default();
        let tol = 1e-5;
        let (fillet, end) = bumped_rib_surfaces(1e-3);
        let rib = tangent_end_rib(
            &k,
            &fillet,
            &end,
            2,
            Point3::new(-0.5, 0.0, 0.0),
            Point3::new(0.5, 1.0, 0.0),
            None,
            false,
            tol,
        )
        .unwrap();
        let worst = farthest_off(&k, &rib, &fillet, &end);
        assert!(
            worst <= tol,
            "the rib is {worst:e} off a surface (tolerance {tol:e})"
        );
        let bound = rib.bound.unwrap();
        assert!(bound <= tol, "{bound:e}");
        assert!(worst <= bound + 1e-12, "{worst:e} > {bound:e}");
        let same = same_parameter_off(&k, &rib, &fillet, &end, &|t| on_bumped_rib(&k, &end, t));
        assert!(same <= bound + 1e-12, "{same:e} > {bound:e}");
    }

    /// The bumped surfaces' rib point at `t`: the section is `y = t` (the fillet surface's
    /// u), on the end surface at `(t, 1/2)` (its `y` is `u`, its `z` is `2v − 1`).
    fn on_bumped_rib(k: &Kernel, end: &Surface, t: f64) -> Point3 {
        end.evaluate_point(k, Point2::new(t, 0.5)).unwrap()
    }

    /// The farthest each of the rib's curves is from the rib point `exact(t)` at the same
    /// parameter, at 1001 parameters: the 3D curve's point, and each UV curve's point on its
    /// surface. The bound claims this distance.
    fn same_parameter_off(
        k: &Kernel,
        rib: &TangentEndRib,
        fillet: &Surface,
        end: &Surface,
        exact: &dyn Fn(f64) -> Point3,
    ) -> f64 {
        let ivl = rib.curve_3d.natural_interval();
        let on = |c: &BSplineCurve, s: &Surface, t: f64| {
            let p = c.evaluate_point(t).unwrap();
            s.evaluate_point(k, Point2::new(p.x, p.y)).unwrap()
        };
        let mut worst: f64 = 0.0;
        for i in 0..=1000 {
            let t = ivl.min() + (ivl.max() - ivl.min()) * f64::from(i) / 1000.0;
            let x = exact(t);
            for y in [
                rib.curve_3d.evaluate_point(t).unwrap(),
                on(&rib.fillet_uv, fillet, t),
                on(&rib.end_uv, end, t),
            ] {
                worst = worst.max(y.distance(x));
            }
        }
        worst
    }

    /// The farthest the rib's curves are from the surfaces, at 1001 parameters: the 3D
    /// curve from either surface, the fillet UV curve's point from the end surface, the end
    /// UV curve's point from the fillet surface. Each is at most the curve's distance from
    /// the rib point at the same parameter, which lies on both.
    fn farthest_off(k: &Kernel, rib: &TangentEndRib, fillet: &Surface, end: &Surface) -> f64 {
        let opts = DropPointOptions::default();
        let ivl = rib.curve_3d.natural_interval();
        let on = |c: &BSplineCurve, s: &Surface, t: f64| {
            let p = c.evaluate_point(t).unwrap();
            s.evaluate_point(k, Point2::new(p.x, p.y)).unwrap()
        };
        let mut worst: f64 = 0.0;
        for i in 0..=1000 {
            let t = ivl.min() + (ivl.max() - ivl.min()) * f64::from(i) / 1000.0;
            let x = rib.curve_3d.evaluate_point(t).unwrap();
            let points = [
                (x, fillet),
                (x, end),
                (on(&rib.fillet_uv, fillet, t), end),
                (on(&rib.end_uv, end, t), fillet),
            ];
            for (x, s) in points {
                let dom = s.natural_uv_domain_with_nurb(k).unwrap();
                let d = s
                    .drop_point::<crate::surface::surface::p4::NoObject>(k, x, &dom, &opts)
                    .unwrap();
                assert!(d.success);
                worst = worst.max(d.gap);
            }
        }
        worst
    }

    /// A face the certificate does not enclose (#17744): the clean rib's fillet plane as an
    /// offset surface (0.1 off, the plane `z = c`), the rib the line `x = y − 1/2` there.
    /// It is built as `main` builds it, on its spans' midpoints, and claims no bound.
    #[test]
    fn a_rib_on_a_face_not_enclosed_is_built_without_a_bound() {
        let k = Kernel::default();
        let (plane, end) = bumped_rib_surfaces(0.0);
        let fillet = Surface::from(
            crate::surface::offset_surface::OffsetSurface::new(
                &k,
                0.1,
                plane,
                true,
                crate::surface::offset_surface::SS_UNKNOWN,
            )
            .unwrap(),
        );
        let c = fillet.evaluate_point(&k, Point2::new(0.5, 0.5)).unwrap().z;
        let (p, q) = (Point3::new(-0.5, 0.0, c), Point3::new(0.5, 1.0, c));
        let tol = 1e-5;
        let rib = tangent_end_rib(&k, &fillet, &end, 2, p, q, None, false, tol).unwrap();
        assert_eq!(rib.bound, None);
        let worst = farthest_off(&k, &rib, &fillet, &end);
        assert!(worst <= tol, "{worst:e}");
    }

    /// The same surfaces without the bump: the rib is the line `x = y − 1/2` on `z = 0`. It
    /// is certified, the tangent end's span included ([`certificate`]'s graded pieces and
    /// its bound near the end), with a stored bound within the tolerance that holds the
    /// sampled distances (the rib lies on both surfaces, so a curve point's distance from
    /// one is at most its distance from the rib point; 1e-12 for the projection's rounding).
    #[test]
    fn a_rib_on_its_surfaces_is_certified_within_the_tolerance() {
        let k = Kernel::default();
        let tol = 1e-5;
        let (fillet, end) = bumped_rib_surfaces(0.0);
        let rib = tangent_end_rib(
            &k,
            &fillet,
            &end,
            2,
            Point3::new(-0.5, 0.0, 0.0),
            Point3::new(0.5, 1.0, 0.0),
            None,
            false,
            tol,
        )
        .unwrap();
        let bound = rib.bound.unwrap();
        assert!(bound <= tol, "{bound:e}");
        let worst = farthest_off(&k, &rib, &fillet, &end);
        assert!(worst <= bound + 1e-12, "{worst:e} > {bound:e}");
        let same = same_parameter_off(&k, &rib, &fillet, &end, &|t| on_bumped_rib(&k, &end, t));
        assert!(same <= bound + 1e-12, "{same:e} > {bound:e}");
    }

    /// A quarter cylinder of radius `r` round the y axis lifted by `r`, its section along u
    /// (the rail u = 0 the line `x = z = 0`, u = 1 the line `x = z = r`), and an end surface
    /// `z = (x² / 2 + 2 y²) / (2 r)`, biquadratic, that touches it at the origin with a
    /// curvature across the section half the cylinder's: a true tangent end of contact
    /// order 2, the rib `y ≈ x / 2` from it. Returned with the rib's other end.
    fn tangent_end_surfaces(r: f64) -> (Surface, Surface, Point3) {
        let p = Point3::new;
        let w = core::f64::consts::FRAC_1_SQRT_2;
        let (y0, y1) = (-r, r);
        let mut pts = Vec::new();
        for q in [p(0.0, 0.0, 0.0), p(r, 0.0, 0.0), p(r, 0.0, r)] {
            pts.push(p(q.x, y0, q.z));
            pts.push(p(q.x, y1, q.z));
        }
        let fillet = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 2,
            v_degree: 1,
            control_points: &pts,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &[3, 3],
            v_multiplicities: &[2, 2],
            u_knots: &[0.0, 1.0],
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: Some(&[1.0, 1.0, w, w, 1.0, 1.0]),
            uv_domain: None,
        })
        .unwrap();
        // z = c x² + b y² over x in [−r/4, 5r/4], y in [−r, r]: a quadratic's Bernstein
        // coefficients on [x0, x0 + h] are c x0², c x0 (x0 + h), c (x0 + h)²
        let (c, b) = (1.0 / (4.0 * r), 1.0 / r);
        let bern =
            |k: f64, x0: f64, h: f64| [k * x0 * x0, k * x0 * (x0 + h), k * (x0 + h) * (x0 + h)];
        let (x0, hx, hy) = (-0.25 * r, 1.5 * r, 2.0 * r);
        let (zx, zy) = (bern(c, x0, hx), bern(b, y0, hy));
        let mut pts = Vec::new();
        let halves = [0.0, 0.5, 1.0];
        for (fx, zi) in halves.iter().zip(zx) {
            for (fy, zj) in halves.iter().zip(zy) {
                pts.push(p(x0 + hx * fx, y0 + hy * fy, zi + zj));
            }
        }
        let end = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 2,
            v_degree: 2,
            control_points: &pts,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &[3, 3],
            v_multiplicities: &[3, 3],
            u_knots: &[0.0, 1.0],
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: None,
            uv_domain: None,
        })
        .unwrap();
        // the other rail: x = z = r, on the end surface where b y² = r − c r²
        let other = p(r, ((r - c * r * r) / b).sqrt(), r);
        (Surface::from(fillet), Surface::from(end), other)
    }

    /// The third outcome of the tangent end's span (#17744, the maintainer's answer on
    /// #18674): on a true tangent end of a large round (radius 40), both faces enclosed
    /// (NURBS), the certificate's graded pieces stop at `t*` near the tangent end, where the
    /// bound over `[0, t*]` is not within the tolerance. The span is accepted on `main`'s
    /// midpoint check, every other span certified, and the rib makes no claim.
    #[test]
    fn a_rib_whose_tangent_end_is_not_bounded_is_built_without_a_bound() {
        let k = Kernel::default();
        let tol = 1e-5;
        let (fillet, end, other) = tangent_end_surfaces(40.0);
        assert!(certificate::Face::of(&k, &fillet).unwrap().is_some());
        assert!(certificate::Face::of(&k, &end).unwrap().is_some());
        let p = Point3::new(0.0, 0.0, 0.0);
        let rib = tangent_end_rib(&k, &fillet, &end, 2, p, other, None, false, tol).unwrap();
        assert_eq!(rib.bound, None);
        let worst = farthest_off(&k, &rib, &fillet, &end);
        assert!(worst <= tol, "{worst:e}");
    }

    /// A rib on a face not enclosed keeps upstream's points (bherbruck on #18674): only a
    /// rib on enclosed faces settles them ([`settled_point`]). The large round of
    /// [`tangent_end_surfaces`] (radius 40) with its end surface as a zero offset of itself,
    /// which is not enclosed: at each station the 3D curve passes through the point
    /// [`tangent_end_rib_point`] solves from upstream's guess (within 1e-13), as on `main`, where
    /// settling would have moved a station by more than 1e-12.
    #[test]
    fn a_rib_on_a_face_not_enclosed_keeps_upstreams_points() {
        use crate::surface::offset_surface::{OffsetSurface, SS_UNKNOWN};
        let k = Kernel::default();
        let tol = 1e-5;
        let (fillet, base, other) = tangent_end_surfaces(40.0);
        let end = Surface::from(OffsetSurface::new(&k, 0.0, base, true, SS_UNKNOWN).unwrap());
        assert!(certificate::Face::of(&k, &end).unwrap().is_none());
        let p = Point3::new(0.0, 0.0, 0.0);
        let rib = tangent_end_rib(&k, &fillet, &end, 2, p, other, None, false, tol).unwrap();
        assert_eq!(rib.bound, None);
        // upstream's stations t = j / n (a cubic through n + 1 points has n + 1 control
        // points), each solved from the chord's guess, then the last two points carried
        // on, the last from the other end's
        let n = rib.curve_3d.number_control_points().unwrap() - 1;
        let opts = DropPointOptions::default();
        let drop = |s: &Surface, x: Point3| {
            let dom = s.natural_uv_domain_with_nurb(&k).unwrap();
            s.drop_point::<crate::surface::surface::p4::NoObject>(&k, x, &dom, &opts)
                .unwrap()
                .result_uv
        };
        let (f0, f1, e0, e1) = (
            drop(&fillet, p),
            drop(&fillet, other),
            drop(&end, p),
            drop(&end, other),
        );
        // the section along u: sigma is u, alpha v
        assert!(
            f0.x.abs() < 1e-9 && (f1.x - 1.0).abs() < 1e-9,
            "{f0:?} {f1:?}"
        );
        let (mut alphas, mut es) = (vec![f0.y], vec![e0]);
        let (mut off_main, mut moved) = (0.0_f64, 0.0_f64);
        for j in 1..=n {
            #[allow(clippy::cast_precision_loss)]
            let t = j as f64 / n as f64;
            let (alpha, uv) = if j == n {
                (f1.y, e1)
            } else if j == 1 {
                (
                    f0.y + (f1.y - f0.y) * t,
                    Point2::new(e0.x + (e1.x - e0.x) * t, e0.y + (e1.y - e0.y) * t),
                )
            } else {
                let (a1, a2, u1, u2) = (alphas[j - 1], alphas[j - 2], es[j - 1], es[j - 2]);
                (
                    2.0 * a1 - a2,
                    Point2::new(2.0 * u1.x - u2.x, 2.0 * u1.y - u2.y),
                )
            };
            let sigma = f0.x + (f1.x - f0.x) * t;
            let point = tangent_end_rib_point(&k, &fillet, &end, 0, sigma, alpha, uv)
                .unwrap()
                .unwrap();
            let (a, e, x) = point;
            alphas.push(a);
            es.push(e);
            let settled = settled_point(&k, &fillet, &end, 0, sigma, point).unwrap();
            moved = moved.max(settled.2.distance(x));
            if j < n {
                off_main = off_main.max(rib.curve_3d.evaluate_point(t).unwrap().distance(x));
            }
        }
        println!("stations {n}: off main's points {off_main:e}, settling moves {moved:e}");
        assert!(off_main <= 1e-13, "{off_main:e}");
        assert!(moved > 1e-12, "{moved:e}");
    }

    /// The rib point at `t` of [`tangent_end_surfaces`]: the section `sigma = t` gives the
    /// cylinder's `x` and `z`, and the end surface `y = sqrt(r z − x² / 4)`.
    fn on_tangent_rib(k: &Kernel, fillet: &Surface, r: f64, t: f64) -> Point3 {
        let q = fillet.evaluate_point(k, Point2::new(t, 0.5)).unwrap();
        Point3::new(q.x, (r * q.z - q.x * q.x / 4.0).max(0.0).sqrt(), q.z)
    }

    /// A true tangent end of contact order 2 on a small round (radius 1, the surfaces of
    /// [`tangent_end_surfaces`]): the first span is certified to the tangent end, its
    /// `[0, t*]` bounded without the Jacobian, and the bound holds each curve's distance
    /// from the closed-form rib at the same parameter.
    #[test]
    fn a_rib_at_a_true_tangent_end_is_certified_within_the_tolerance() {
        let k = Kernel::default();
        let tol = 1e-5;
        let r = 1.0;
        let (fillet, end, other) = tangent_end_surfaces(r);
        let p = Point3::new(0.0, 0.0, 0.0);
        let rib = tangent_end_rib(&k, &fillet, &end, 2, p, other, None, false, tol).unwrap();
        let bound = rib.bound.unwrap();
        assert!(bound <= tol, "{bound:e}");
        let same = same_parameter_off(&k, &rib, &fillet, &end, &|t| {
            on_tangent_rib(&k, &fillet, r, t)
        });
        assert!(same <= bound + 1e-12, "{same:e} > {bound:e}");
    }

    /// An end surface with a kink: `x = y − 1/2 + d(y)`, `d` the tent of height 0.1 at
    /// `y = 0.4`, degree 1, so the rib bends there by a slope step of 0.42 and 0.4 is never
    /// a station (the stations are dyadic). No station count bounds the cubic curves
    /// within 1e-6 across the kink, and the rib is refused as not certified.
    #[test]
    fn a_rib_no_station_count_certifies_is_refused() {
        let k = Kernel::default();
        let p = Point3::new;
        let fillet = BSplineSurface::create_bilinear_surface(
            p(-1.0, 0.0, 0.0),
            p(-1.0, 1.0, 0.0),
            p(1.0, 0.0, 0.0),
            p(1.0, 1.0, 0.0),
        )
        .unwrap();
        let mut pts = Vec::new();
        for (x, y) in [(-0.5, 0.0), (0.0, 0.4), (0.5, 1.0)] {
            pts.push(p(x, y, -1.0));
            pts.push(p(x, y, 1.0));
        }
        let end = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 1,
            v_degree: 1,
            control_points: &pts,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &[2, 1, 2],
            v_multiplicities: &[2, 2],
            u_knots: &[0.0, 0.4, 1.0],
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: None,
            uv_domain: None,
        })
        .unwrap();
        let (fillet, end) = (Surface::from(fillet), Surface::from(end));
        let e = tangent_end_rib(
            &k,
            &fillet,
            &end,
            2,
            p(-0.5, 0.0, 0.0),
            p(0.5, 1.0, 0.0),
            None,
            false,
            1e-6,
        )
        .unwrap_err();
        assert_eq!(e.operation(), OP_RIB);
        assert!(
            matches!(e.blend_failure(), Some(BlendFailure::NotCertified { .. })),
            "{e:?}"
        );
    }

    /// The certificate's loops poll the kernel's cancellation: the clean rib cancelled at
    /// its first poll, half way and at its last returns cancelled every time.
    #[test]
    fn a_rib_cancelled_while_it_is_certified_returns_cancelled() {
        use crate::kernel::CancelToken;
        let (fillet, end) = bumped_rib_surfaces(0.0);
        let run = |n: u64| {
            let mut k = Kernel::default();
            let token = CancelToken::after_polls(n);
            k.set_cancel_token(token.clone());
            let r = tangent_end_rib(
                &k,
                &fillet,
                &end,
                2,
                Point3::new(-0.5, 0.0, 0.0),
                Point3::new(0.5, 1.0, 0.0),
                None,
                false,
                1e-5,
            );
            (token.polls().unwrap(), r)
        };
        let (all, r) = run(u64::MAX);
        assert!(r.is_ok());
        assert!(all > 2, "the certificate polls {all} times");
        for n in [1, all / 2, all] {
            let (_, r) = run(n);
            assert!(
                r.is_err_and(|e| e.is_cancelled()),
                "cancelled at poll {n} of {all}"
            );
        }
    }
}
