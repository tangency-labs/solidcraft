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
//! | `EaseRamp` (private) | `SmEaseRamp` |
//! | `split_bezier` (private) | `sm_SplitBezier` |
//! | `restrict_bezier` (private) | `sm_RestrictBezier` |
//! | `ease_law_on_span` (private) | `sm_EaseLawOnSpan` |
//! | `binomial` (private) | `sm_Binomial` |
//! | `raise_knot` (private) | `sm_RaiseKnot` |
//! | `ease_surface` (private) | `sm_EaseSurface` |
//! | `SessionObjects::ease_set_back_sections` (private) | `SmFilletExecutive::EaseSetBackSections` |
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
use crate::blend::fillet_geom::p1::{FilletGeomId, FilletVertexType, SurfaceHandle};
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
use crate::math::{Point3, EFF_ZERO};
use crate::nurbs::b_spline_curve::{BSplineCurve, KnotType};
use crate::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData, SurfParamType,
};
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

/// `SmEaseRamp`: a stretch of a fillet surface's rail parameter over which its sections ease:
/// `w = 0` at `from` (the round's own section), `w = 1` at `to` (the curvature-continuous
/// one), by the C2 law `w(s) = 10 s^3 - 15 s^4 + 6 s^5` (its Bernstein coefficients
/// `0, 0, 0, 1, 1, 1`), which is flat to second order at both ends; `w` stays 1 beyond `to`.
#[derive(Debug, Clone, Copy)]
struct EaseRamp {
    from: f64,
    to: f64,
}

fn at(v: &[f64], i: usize) -> Result<f64> {
    v.get(i).copied().ok_or_else(sm_err)
}

fn put(v: &mut [f64], i: usize, x: f64) -> Result<()> {
    *v.get_mut(i).ok_or_else(sm_err)? = x;
    Ok(())
}

/// `sm_SplitBezier`: the Bezier coefficients `coef` (degree `coef.len() - 1`) split at `t`
/// by de Casteljau: the left part on `[0, t]`, the right on `[t, 1]`.
fn split_bezier(coef: &[f64], t: f64) -> Result<(Vec<f64>, Vec<f64>)> {
    let n = coef.len().checked_sub(1).ok_or_else(sm_err)?;
    let mut work = coef.to_vec();
    let mut left = coef.to_vec();
    let mut right = coef.to_vec();
    put(&mut left, 0, at(&work, 0)?)?;
    put(&mut right, n, at(&work, n)?)?;
    for k in 1..=n {
        for j in 0..=(n - k) {
            let x = (1.0 - t) * at(&work, j)? + t * at(&work, j + 1)?;
            put(&mut work, j, x)?;
        }
        put(&mut left, k, at(&work, 0)?)?;
        put(&mut right, n - k, at(&work, n - k)?)?;
    }
    Ok((left, right))
}

/// `sm_RestrictBezier`: the Bernstein coefficients on `[s0, s1]` (`0 <= s0 < s1 <= 1`) of the
/// polynomial whose coefficients on `[0, 1]` are `coef`: its part left of `s1`, then that
/// part's right of `s0 / s1`.
fn restrict_bezier(coef: &mut Vec<f64>, s0: f64, s1: f64) -> Result<()> {
    let (left, _) = split_bezier(coef, s1)?;
    let (_, right2) = split_bezier(&left, if s1 > 0.0 { s0 / s1 } else { 0.0 })?;
    *coef = right2;
    Ok(())
}

/// `sm_EaseLawOnSpan`: the law's Bernstein coefficients (degree 5) on the span `[a, b]` of the
/// rail parameter, given the ramps: on a span inside one ramp its smoothstep restricted
/// there; elsewhere the constant `w` is 1 on the side of a ramp's `to`, 0 on the side of its
/// `from`, the largest over the ramps. A span where two ramps disagree (one easing, another
/// already 1) is refused (`SM_ERR`).
fn ease_law_on_span(ramps: &[EaseRamp], a: f64, b: f64) -> Result<Vec<f64>> {
    let mut coef = vec![0.0; 6];
    let mut constant = 0.0;
    let mut easing = false;
    let m = 0.5 * (a + b);
    for ramp in ramps {
        let lo = ramp.from.min(ramp.to);
        let hi = ramp.from.max(ramp.to);
        let rising = ramp.from < ramp.to;
        if m > lo && m < hi {
            if easing {
                return Err(sm_err()); // two ramps over one span
            }
            easing = true;
            let mut ramp_coef: Vec<f64> = (0..6)
                .map(|i| if (i >= 3) == rising { 1.0 } else { 0.0 })
                .collect();
            restrict_bezier(&mut ramp_coef, (a - lo) / (hi - lo), (b - lo) / (hi - lo))?;
            coef = ramp_coef;
        } else {
            let beyond_to = if rising { m >= hi } else { m <= lo };
            if beyond_to {
                constant = 1.0;
            }
        }
    }
    if easing {
        if constant != 0.0 {
            return Err(sm_err()); // a ramp easing where another is already through
        }
        return Ok(coef);
    }
    coef.fill(constant);
    Ok(coef)
}

/// `sm_Binomial`: `n` choose `k` as a double, by the running product.
fn binomial(n: usize, k: usize) -> Result<f64> {
    let mut d = 1.0;
    for i in 1..=k {
        let top = (n + i).checked_sub(k).ok_or_else(sm_err)?;
        d = d * top as f64 / i as f64;
    }
    Ok(d)
}

/// `sm_RaiseKnot`: inserts into `surf`, in direction `dir`, the knot `knot` up to
/// multiplicity `mult` (counting knots within `tol` of it as the same).
fn raise_knot(
    surf: &mut BSplineSurface,
    dir: SurfParamType,
    knot: f64,
    mult: usize,
    tol: f64,
) -> Result<()> {
    let canonical = surf.canonical()?;
    let (knots, mults) = if dir == SurfParamType::U {
        (&canonical.u_knots, &canonical.u_multiplicities)
    } else {
        (&canonical.v_knots, &canonical.v_multiplicities)
    };
    let mut have = 0;
    let mut knot = knot;
    for (i, &existing) in knots.iter().enumerate() {
        if (existing - knot).abs() <= tol {
            have = mults.get(i).copied().ok_or_else(sm_err)?;
            knot = existing;
        }
    }
    if have < mult {
        surf.insert_one_knot(knot, mult - have, dir)?;
    }
    Ok(())
}

/// The interval of `domain` in `dir` (u for [`SurfParamType::U`], else v).
fn interval_in(domain: &Extent2d, dir: SurfParamType) -> crate::geom::extent1d::Extent1d {
    if dir == SurfParamType::U {
        domain.u_interval()
    } else {
        domain.v_interval()
    }
}

/// The UV point at `along` in the rails' direction `dir` and `across` in the other.
fn uv_at(dir: SurfParamType, along: f64, across: f64) -> Point2 {
    if dir == SurfParamType::U {
        Point2::new(along, across)
    } else {
        Point2::new(across, along)
    }
}

/// `sm_EaseSurface`: the surface whose sections ease from `from`'s to `to`'s over the ramps
/// along `along`, the rails' direction, in which both run over the same interval with the
/// same rail parameter: `P = (1 - w) From + w To` in homogeneous coordinates, `w` the C2 law,
/// so `P` is `From` to second order where `w` is 0 and `To` where it is 1. Exact: both
/// surfaces are raised to common degrees and knots, put in Bezier form along the rails, and
/// each span's net multiplied by the law's Bernstein form. The span ends keep their knots at
/// the new degree's multiplicity.
///
/// # Errors
/// `SM_ERR` where the fork `SER(SM_ERR)`s: other rail intervals, a turned `to` that does not
/// turn, a ramp end outside the surface, nets that do not match, an interior knot below full
/// multiplicity, two ramps over one span or a weight not above `SM_EFF_ZERO`; those of the
/// surface operations.
#[allow(clippy::too_many_lines)] // one fork routine
fn ease_surface(
    from: &BSplineSurface,
    to: &BSplineSurface,
    along: SurfParamType,
    ramps: &[EaseRamp],
) -> Result<BSplineSurface> {
    let across = if along == SurfParamType::U {
        SurfParamType::V
    } else {
        SurfParamType::U
    };
    let mut a = from.clone();
    let mut b = to.clone();

    // the same domain: along, the rail parameter in both; across, crFrom's
    let dom_a = a.natural_uv_domain();
    let dom_b = b.natural_uv_domain();
    let along_a = interval_in(&dom_a, along);
    let along_b = interval_in(&dom_b, along);
    let param_tol = 1.0e-9 * (1.0 + along_a.length());
    if (along_a.min() - along_b.min()).abs() > param_tol
        || (along_a.max() - along_b.max()).abs() > param_tol
    {
        return Err(sm_err());
    }
    let across_a = interval_in(&dom_a, across);
    let new_b = Extent2d::from_points(
        uv_at(along, along_a.min(), across_a.min()),
        uv_at(along, along_a.max(), across_a.max()),
    );
    b.reparameterize(&new_b)?;
    // the same rail at the across direction's start in both: crTo's turned if it runs the
    // other way
    {
        let t = along_a.evaluate(0.5);
        let at0 = uv_at(along, t, across_a.min());
        let at1 = uv_at(along, t, across_a.max());
        let a0 = a.evaluate_point(at0)?;
        let b0 = b.evaluate_point(at0)?;
        let b1 = b.evaluate_point(at1)?;
        if a0.distance(b1) < a0.distance(b0) {
            b.reverse(across)?;
            b.reparameterize(&new_b)?;
            let now = b.evaluate_point(at0)?;
            if now.distance(b1) > 1.0e-9 * (1.0 + b0.distance(b1)) {
                return Err(sm_err());
            }
        }
    }

    // common degrees, the law's 5 added along
    let along_deg = a.degree(along).max(b.degree(along));
    let across_deg = a.degree(across).max(b.degree(across));
    for s in [&mut a, &mut b] {
        if s.degree(along) < along_deg {
            s.degree_elevate(along, along_deg)?;
        }
        if s.degree(across) < across_deg {
            s.degree_elevate(across, across_deg)?;
        }
    }

    // common knots: each surface gets the other's; along, every knot and the ramps' ends
    // raised to the degree (Bezier form)
    for pass in 0..2 {
        let other = if pass == 0 {
            b.canonical()?
        } else {
            a.canonical()?
        };
        let s = if pass == 0 { &mut a } else { &mut b };
        let inner = |n: usize| 1..n.saturating_sub(1);
        for i in inner(other.u_knots.len()) {
            let knot = at(&other.u_knots, i)?;
            let mult = other.u_multiplicities.get(i).copied().ok_or_else(sm_err)?;
            raise_knot(s, SurfParamType::U, knot, mult, param_tol)?;
        }
        for i in inner(other.v_knots.len()) {
            let knot = at(&other.v_knots, i)?;
            let mult = other.v_multiplicities.get(i).copied().ok_or_else(sm_err)?;
            raise_knot(s, SurfParamType::V, knot, mult, param_tol)?;
        }
    }
    for s in [&mut a, &mut b] {
        for ramp in ramps {
            for d in [ramp.from, ramp.to] {
                // a ramp ends inside the surface or on its end (the setback, where the trace
                // stopped)
                if d < along_a.min() - param_tol || d > along_a.max() + param_tol {
                    return Err(sm_err());
                }
                if d > along_a.min() + param_tol && d < along_a.max() - param_tol {
                    raise_knot(s, along, d, along_deg, param_tol)?;
                }
            }
        }
    }
    for s in [&mut a, &mut b] {
        let canonical = s.canonical()?;
        let knots = if along == SurfParamType::U {
            canonical.u_knots
        } else {
            canonical.v_knots
        };
        for i in 1..knots.len().saturating_sub(1) {
            raise_knot(s, along, at(&knots, i)?, along_deg, param_tol)?;
        }
    }

    // the two nets, which now have the same structure
    let ca = a.canonical()?;
    let cb = b.canonical()?;
    if ca.u_degree != cb.u_degree
        || ca.v_degree != cb.v_degree
        || ca.control_points.len() != cb.control_points.len()
        || ca.u_knots.len() != cb.u_knots.len()
        || ca.v_knots.len() != cb.v_knots.len()
    {
        return Err(sm_err());
    }
    for (i, (&ka, &kb)) in ca.u_knots.iter().zip(&cb.u_knots).enumerate() {
        if ca.u_multiplicities.get(i) != cb.u_multiplicities.get(i) || (ka - kb).abs() > param_tol {
            return Err(sm_err());
        }
    }
    for (i, (&ka, &kb)) in ca.v_knots.iter().zip(&cb.v_knots).enumerate() {
        if ca.v_multiplicities.get(i) != cb.v_multiplicities.get(i) || (ka - kb).abs() > param_tol {
            return Err(sm_err());
        }
    }
    let rational = !ca.weights.is_empty() || !cb.weights.is_empty();
    let (along_knots, along_mult, across_knots, across_mult) = if along == SurfParamType::U {
        (
            &ca.u_knots,
            &ca.u_multiplicities,
            &ca.v_knots,
            &ca.v_multiplicities,
        )
    } else {
        (
            &ca.v_knots,
            &ca.v_multiplicities,
            &ca.u_knots,
            &ca.u_multiplicities,
        )
    };
    let spans = along_knots.len().checked_sub(1).ok_or_else(sm_err)?;
    let across_count = across_mult
        .iter()
        .sum::<usize>()
        .checked_sub(across_deg + 1)
        .ok_or_else(sm_err)?;
    let along_count = spans * along_deg + 1;
    if along_count * across_count != ca.control_points.len() {
        return Err(sm_err());
    }
    for i in 1..spans {
        if along_mult.get(i).copied() != Some(along_deg) {
            return Err(sm_err());
        }
    }
    // the homogeneous control point [along i][across j] of a net (U-major lists)
    let homog = |pts: &[Point3], w: &[f64], i: usize, j: usize| -> Result<[f64; 4]> {
        let k = if along == SurfParamType::U {
            i * across_count + j
        } else {
            j * along_count + i
        };
        let w = if w.is_empty() { 1.0 } else { at(w, k)? };
        let p = pts.get(k).copied().ok_or_else(sm_err)?;
        Ok([p.x * w, p.y * w, p.z * w, w])
    };

    // each span's product with the law
    let p = along_deg;
    let new_deg = p + 5;
    let new_along_count = spans * new_deg + 1;
    let mut net = vec![0.0; new_along_count * across_count * 4];
    for k in 0..spans {
        let w = ease_law_on_span(ramps, at(along_knots, k)?, at(along_knots, k + 1)?)?;
        for j in 0..across_count {
            for m in 0..=new_deg {
                let mut h = [0.0; 4];
                let i_min = m.saturating_sub(5);
                let i_max = p.min(m);
                for i in i_min..=i_max {
                    let l = m - i;
                    let beta = binomial(p, i)? * binomial(5, l)? / binomial(new_deg, m)?;
                    let ha = homog(&ca.control_points, &ca.weights, k * p + i, j)?;
                    let hb = homog(&cb.control_points, &cb.weights, k * p + i, j)?;
                    let wl = at(&w, l)?;
                    for c in 0..4 {
                        let (ha_c, hb_c) = (
                            ha.get(c).copied().ok_or_else(sm_err)?,
                            hb.get(c).copied().ok_or_else(sm_err)?,
                        );
                        let hc = h.get_mut(c).ok_or_else(sm_err)?;
                        *hc += beta * ((1.0 - wl) * ha_c + wl * hb_c);
                    }
                }
                let row = k * new_deg + m;
                for (c, &hc) in h.iter().enumerate() {
                    put(&mut net, (row * across_count + j) * 4 + c, hc)?;
                }
            }
        }
    }

    // the eased surface, back in Euclidean form
    let mut new_pts = vec![Point3::new(0.0, 0.0, 0.0); new_along_count * across_count];
    let mut new_weights = if rational {
        vec![1.0; new_along_count * across_count]
    } else {
        Vec::new()
    };
    for i in 0..new_along_count {
        for j in 0..across_count {
            let base = (i * across_count + j) * 4;
            let h = [
                at(&net, base)?,
                at(&net, base + 1)?,
                at(&net, base + 2)?,
                at(&net, base + 3)?,
            ];
            if !(h[3] > EFF_ZERO) {
                return Err(sm_err());
            }
            let k = if along == SurfParamType::U {
                i * across_count + j
            } else {
                j * new_along_count + i
            };
            *new_pts.get_mut(k).ok_or_else(sm_err)? =
                Point3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]);
            if rational {
                put(&mut new_weights, k, h[3])?;
            }
        }
    }
    let new_along_mult: Vec<usize> = (0..=spans)
        .map(|i| {
            if i == 0 || i == spans {
                new_deg + 1
            } else {
                new_deg
            }
        })
        .collect();
    let weights = if rational {
        Some(new_weights.as_slice())
    } else {
        None
    };
    let data = if along == SurfParamType::U {
        CanonicalSurfaceData {
            u_degree: new_deg,
            v_degree: across_deg,
            control_points: &new_pts,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &new_along_mult,
            v_multiplicities: across_mult,
            u_knots: along_knots,
            v_knots: across_knots,
            knot_type: KnotType::Unspecified,
            weights,
            uv_domain: None,
        }
    } else {
        CanonicalSurfaceData {
            u_degree: across_deg,
            v_degree: new_deg,
            control_points: &new_pts,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: across_mult,
            v_multiplicities: &new_along_mult,
            u_knots: across_knots,
            v_knots: along_knots,
            knot_type: KnotType::Unspecified,
            weights,
            uv_domain: None,
        }
    };
    BSplineSurface::create_canonical(&data)
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
            let g2 = g2_generator.create_surface_from_curves(
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
            )?;

            let eased = ease_surface(&own, &g2, along, geom_ramps)?;
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

    fn smoothstep(s: f64) -> f64 {
        10.0 * s * s * s - 15.0 * s * s * s * s + 6.0 * s * s * s * s * s
    }

    fn bernstein(coef: &[f64], s: f64) -> f64 {
        let n = coef.len() - 1;
        coef.iter()
            .enumerate()
            .map(|(i, c)| {
                c * binomial(n, i).unwrap() * s.powi(i as i32) * (1.0 - s).powi((n - i) as i32)
            })
            .sum()
    }

    #[test]
    fn the_law_restricted_to_a_span_is_the_smoothstep_there() {
        let ramps = [EaseRamp { from: 2.0, to: 6.0 }];
        let coef = ease_law_on_span(&ramps, 3.0, 5.0).unwrap();
        for s in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let x = 3.0 + 2.0 * s;
            let w = smoothstep((x - 2.0) / 4.0);
            assert!((bernstein(&coef, s) - w).abs() < 1e-14, "{s}");
        }
        // falling ramp: 1 at its `to` (the smaller parameter)
        let ramps = [EaseRamp { from: 6.0, to: 2.0 }];
        let coef = ease_law_on_span(&ramps, 2.0, 6.0).unwrap();
        assert!((bernstein(&coef, 0.0) - 1.0).abs() < 1e-15);
        assert!(bernstein(&coef, 1.0).abs() < 1e-15);
        // beyond `to` the law is 1, before `from` 0
        assert_eq!(ease_law_on_span(&ramps, 0.0, 2.0).unwrap(), vec![1.0; 6]);
        assert_eq!(ease_law_on_span(&ramps, 6.0, 9.0).unwrap(), vec![0.0; 6]);
    }

    #[test]
    fn two_ramps_over_one_span_are_refused() {
        let ramps = [
            EaseRamp { from: 0.0, to: 4.0 },
            EaseRamp { from: 1.0, to: 5.0 },
        ];
        assert!(ease_law_on_span(&ramps, 2.0, 3.0).is_err());
        // one easing where another is already through
        let ramps = [
            EaseRamp { from: 0.0, to: 1.0 },
            EaseRamp { from: 2.0, to: 4.0 },
        ];
        assert!(ease_law_on_span(&ramps, 2.5, 3.0).is_err());
    }

    #[test]
    fn binomials() {
        assert_eq!(binomial(5, 0).unwrap(), 1.0);
        assert_eq!(binomial(5, 2).unwrap(), 10.0);
        assert_eq!(binomial(8, 4).unwrap(), 70.0);
    }
}
