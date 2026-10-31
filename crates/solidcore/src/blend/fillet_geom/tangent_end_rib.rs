//! The end rib of a fillet at a tangent end, solved from the tangent end outward.
//!
//! Upstream: the SolidCraft fork's `source/SMLib/src/SmFilletGeom.cpp`
//! (tangency-labs/smlib@856b54e; the zero-speed rib in `t` of an odd order past 1,
//! tangency-labs/smlib@cd47efd), under WR-600's
//! `fillet_nx1_tangent_side_edge_same_convexity`.
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

use crate::blend::fillet_solver::p1::{
    FilletSolver, FilletSurfaceGenerator, FilletSurfaceGeneratorType,
};
use crate::error::{Result, SolidError};
use crate::geom::vector2d::Point2;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, EFF_ZERO, EFF_ZERO_SQRT};
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
/// one, `n + 1` for a blend curve of Gn continuity. The section stands off its face as
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
        .ok_or_else(|| SolidError::null_pointer(OP_ORDER))?;
    match fsg.fillet_surface_generator_type() {
        FilletSurfaceGeneratorType::Linear => Ok(1),
        FilletSurfaceGeneratorType::Circular => Ok(2),
        // ((SmBlendCurveCrossSectionFSG*)pFSG)->GetContinuity() + 1
        FilletSurfaceGeneratorType::BlendCurve => match fsg {
            FilletSurfaceGenerator::BlendCurve(g) => g
                .continuity
                .checked_add(1)
                .ok_or_else(|| SolidError::sm_err(OP_ORDER)),
            _ => Err(SolidError::sm_err(OP_ORDER)),
        },
        FilletSurfaceGeneratorType::Unknown => Err(SolidError::sm_err(OP_ORDER)),
    }
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
            .ok_or_else(|| SolidError::sm_err(OP_RIB))
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

/// The curves of [`tangent_end_rib`].
#[derive(Debug, Clone)]
pub struct TangentEndRib {
    /// `rp3DCurve`.
    pub curve_3d: BSplineCurve,
    /// `rpFilletUV`: its UV curve on the fillet surface.
    pub fillet_uv: BSplineCurve,
    /// `rpEndUV`: its UV curve on the end surface.
    pub end_uv: BSplineCurve,
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
/// doubled from 8 to 1024 until each span's midpoint is within `approx_tol` of the
/// exact point there. For odd `k` past 1 (the fork's tangency-labs/smlib@cd47efd) the
/// curves have zero speed at the tangent end: their first derivative there is zero
/// (the first two control points coincide) and their second the exact
/// `2 (s1 - s0) dX/ds`, `X` the fillet surface along its section, both interpolated as
/// end conditions; only the spans whose midpoint is off are halved, from 8 uniform
/// ones. With `reverse` the rib runs from `other_end` to `tangent_end`.
///
/// # Errors
///
/// `SM_ERR` when an end is off either surface, the ends are not on the fillet surface's
/// two rails or lie on one section, the rib does not reach its other end, or no station
/// count gives the rib within tolerance; for a zero-speed rib, when the end surface is
/// singular at the tangent end, the fillet surface's section leaves its tangent plane
/// there, or an interpolation's second control point leaves its end; those of the
/// drops, evaluations and fits.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // one upstream routine
pub fn tangent_end_rib(
    k: &Kernel,
    fillet: &Surface,
    end: &Surface,
    order: u32,
    tangent_end: Point3,
    other_end: Point3,
    reverse: bool,
    approx_tol: f64,
) -> Result<TangentEndRib> {
    let fail = || SolidError::sm_err(OP_RIB);

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
            return Err(fail());
        }
        let e = end.drop_point::<crate::surface::surface::p4::NoObject>(k, p, &end_dom, &opts)?;
        if !e.success || e.gap > approx_tol {
            // an end of the rib is off the end surface
            return Err(fail());
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
        return Err(fail());
    };
    let (sigma0, sigma1, alpha0, alpha1) = if section == 0 {
        (fil0.x, fil1.x, fil0.y, fil1.y)
    } else {
        (fil0.y, fil1.y, fil0.x, fil1.x)
    };
    if (alpha1 - alpha0).abs() <= EFF_ZERO_SQRT * smax(1.0, alpha0.abs()) {
        // the rib's ends lie on one section
        return Err(fail());
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
            return Err(fail());
        }
        let r_u = e_u.dot(d2_x);
        let r_v = e_v.dot(d2_x);
        let d2_end = Vec3::new((c * r_u - b * r_v) / det, (a * r_v - b * r_u) / det, 0.0);
        // X'' lies in the end surface's tangent plane: the fillet surface touches it there
        let off_plane = d2_x - e_u * d2_end.x - e_v * d2_end.y;
        if off_plane.length() > EFF_ZERO_SQRT * smax(1.0, d2_x.length()) {
            // the fillet surface's section leaves the end surface's tangent plane at the
            // tangent end
            return Err(fail());
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
                return Err(fail());
            }
            let (cpt0, _) = c.control_point(ControlPointForm::NonRational, 0)?;
            let (cpt1, _) = c.control_point(ControlPointForm::NonRational, 1)?;
            if cpt1.distance(cpt0) > EFF_ZERO_SQRT * smax(1.0, cpt0.max_dimension()) {
                // the zero-speed end's interpolation leaves its end
                return Err(fail());
            }
            c.set_control_point(ControlPointForm::NonRational, 1, cpt0, 1.0)?;
            return Ok(c);
        }
        BSplineCurve::interpolate_points(pts, Some(ts), 3, None, None, false, ParamMethod::Uniform)
    };

    // the exact rib point at t, from a guess; on the other end's side of the tangent end
    let solve = |t: f64, alpha: f64, uv: Point2| -> Result<Option<(f64, Point2, Point3)>> {
        let found = tangent_end_rib_point(k, fillet, end, section, sigma_at(t), alpha, uv)?;
        // (`<= 0.0` fails the point; a NaN product keeps it, as upstream's test)
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        Ok(found.filter(|&(a, _, _)| !((a - alpha0) * (alpha1 - alpha0) <= 0.0)))
    };

    // The stations, and the exact points there. A regular rib's stations are
    // t = j / spans, the spans doubled until the curves are within the tolerance. A
    // zero-speed rib's are refined where they are not: only the spans whose midpoint is
    // off are halved, at that exact midpoint, so the stations follow the rib's own
    // smoothness in t, the tangent end costing none.
    let mut ts: Vec<f64> = Vec::new();
    let mut alphas: Vec<f64> = Vec::new();
    let mut es: Vec<Point2> = Vec::new();
    let mut xs: Vec<Point3> = Vec::new();
    let max_spans: usize = 1024;
    let mut spans: usize = 8;
    let mut stations = false;
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
                        return Err(fail());
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
            let last = xs.get_mut(spans).ok_or_else(fail)?;
            if last.distance(other_end) > approx_tol {
                // the rib does not reach its other end
                return Err(fail());
            }
            *last = other_end;
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

        // each span's midpoint against the exact rib point there; the spans off it, with
        // that point
        let mut solved = true;
        let mut off: Vec<(usize, f64, Point2, Point3)> = Vec::new();
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
                return Err(fail());
            };
            let t = 0.5 * (t0 + t1);
            let alpha = 0.5 * (a0 + a1);
            let uv = Point2::new(0.5 * (e0.x + e1.x), 0.5 * (e0.y + e1.y));
            let Some((alpha, uv, exact)) = solve(t, alpha, uv)? else {
                solved = false;
                break;
            };
            let on_3d = c3d.evaluate_point(t)?;
            let on_fil_uv = c_fil.evaluate_point(t)?;
            let on_end_uv = c_end.evaluate_point(t)?;
            let on_fil = fillet.evaluate_point(k, Point2::new(on_fil_uv.x, on_fil_uv.y))?;
            let on_end = end.evaluate_point(k, Point2::new(on_end_uv.x, on_end_uv.y))?;
            if on_3d.distance(exact) > approx_tol
                || on_fil.distance(exact) > approx_tol
                || on_end.distance(exact) > approx_tol
            {
                off.push((jj, alpha, uv, exact));
            }
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
            });
        }

        if !zero_speed {
            spans *= 2;
            stations = false;
            continue;
        }

        // the zero-speed rib: each span off the rib halved at its exact midpoint
        if !solved {
            break;
        }
        for &(jj, alpha, uv, exact) in off.iter().rev() {
            let at = jj + 1;
            let (Some(&t0), Some(&t1)) = (ts.get(at - 1), ts.get(at)) else {
                return Err(fail());
            };
            ts.insert(at, 0.5 * (t0 + t1));
            alphas.insert(at, alpha);
            es.insert(at, uv);
            xs.insert(at, exact);
        }
        spans += off.len();
    }

    // no stations give the rib within tolerance
    Err(fail())
}
