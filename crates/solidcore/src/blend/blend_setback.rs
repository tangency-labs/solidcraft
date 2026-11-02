//! The setback corner: where rounds meet at a vertex and stop short of it, each at its
//! setback, the hole they leave is filled by patches that meet the rounds and the faces to
//! the order asked (tangent, G1, or curvature continuous, G2).
//!
//! Not in the pinned upstream: the SolidCraft fork's `source/SMLib/src/SmBlendSetback.cpp`
//! and `source/SMLib/inc/SmBlendSetback.h` (tangency-labs/smlib PR #79: `870c970` the module,
//! the setback distances and the ease moved out of the executive; `beba428` the fill by the
//! setback split; `3fdb76b` a refusal's reason handed back; `52da453` a setback patch leaves its
//! round at the speed of its whole spoke). tangency-labs/solidcore#2273.
//!
//! The pieces, each usable on its own:
//! - [`distances`]: each round's setback, by the owner's rule (the larger of `1.5 r` and the
//!   least setback, where the rails of the rounds beside it cross on their common face, plus
//!   `0.25 r`);
//! - [`ease_surface`]: a round's sections eased from its own into another section (the
//!   curvature continuous one a G2 fill needs) over a stretch of its rails;
//! - [`fill_hole`]: the hole's patches, by the setback split: one setback patch per round,
//!   between its end section, the two faces and an inner curve, and an n-sided interior region
//!   within the inner curves, split into n quadrilaterals at its centre.
//!
//! The interior region is one polynomial map F of a regular n-gon (the domain) into space,
//! and each of its quadrilaterals F composed with the bilinear map of a quarter of the n-gon
//! onto the unit square: the interior parts meet each other as one surface (no conditions
//! between them, at their centre neither). A setback patch, of degree `2k + 1` across the hole
//! (k the order), takes its first `k + 1` rows from the round's own homogeneous derivatives at
//! its end section and its last `k + 1` from F's derivatives across the n-gon's edge, F
//! composed with the map `(u, v) -> e(v) + (1 - u) d(v)` (e the edge, d a field of directions
//! out of the n-gon along it, the two fields at an n-gon vertex opposite): it meets the round
//! to order k in homogeneous coordinates and the interior parts to order k by that map, both
//! exactly. It meets the faces to order k where its first `k + 1` columns lie on the face's
//! plane: on the round's side the round's do (it meets the face to that order), on the
//! interior's side F is held to it, linear conditions on F's coefficients. F is otherwise the
//! one of least bending energy (the sum over the patches of the integral of
//! `|R_uu|^2 + 2 |R_uv|^2 + |R_vv|^2`), at the least degree whose conditions can be met: the
//! conditions are met exactly, the energy only chooses among the maps that meet them.
//!
//! Faces are planes (a curved face is not filled here yet). Every patch lies in the convex
//! hull of its control points (all weights positive), so [`containment`]'s bound is certified.
//!
//! Nothing in the kernel calls [`distances`], [`fill_hole`] or [`containment`] (neither does
//! anything in the fork's SMLib at `5e8c9c1`: only its capi tests do), so they change no
//! existing result. [`ease_surface`] is the fork's `sm_EaseSurface` moved here unchanged; the
//! executive's ease (`FilletSession::set_corner_ease`) calls it as before.
//!
//! | Rust                         | Fork (`SmBlendSetback`)                    |
//! |------------------------------|--------------------------------------------|
//! | [`EaseRamp`]                 | `SmEaseRamp`                               |
//! | [`ease_surface`]             | `SmBlendSetback::EaseSurface`              |
//! | `split_bezier` (private)     | `sm_SplitBezier`                           |
//! | `restrict_bezier` (private)  | `sm_RestrictBezier`                        |
//! | `ease_law_on_span` (private) | `sm_EaseLawOnSpan`                         |
//! | `binomial` (private)         | `sm_Binomial`                              |
//! | `raise_knot` (private)       | `sm_RaiseKnot`                             |
//! | [`Face`], [`Edge`]           | `SmBlendSetback::Face`, `SmBlendSetback::Edge` |
//! | [`distances`], [`Setbacks`]  | `SmBlendSetback::Distances` (its out-params) |
//! | [`Refusal`]                  | a status with `Fill::m_pReason` / `*ppReason` (`SM_SB_FAIL`) |
//! | [`Round`]                    | `SmBlendSetback::Round`                    |
//! | [`Fill`]                     | `SmBlendSetback::Fill`                     |
//! | [`fill_hole`]                | `SmBlendSetback::FillHole`                 |
//! | [`containment`]              | `SmBlendSetback::Containment`              |
//! | `Hom`, `axpy` (private)      | `sm_SbH`, `sm_SbZero`, `sm_SbAxpy`         |
//! | `bern_mul`, `bern_elevate` (private) | `sm_SbMul`, `sm_SbElevate`         |
//! | `Tensor`, `tensor_mul`, `tensor_elevate` (private) | `sm_SbT`, `sm_SbTMul`, `sm_SbTElevate` |
//! | `basis` (private)            | `sm_SbBasis`                               |
//! | `Net`, `second`, `first` (private) | `sm_SbNet`, `sm_SbSecond`, `sm_SbFirst` |
//! | `round_jets` (private)       | `sm_SbRoundJets`                           |
//! | `monomials`, `along_line`, `over_patch` (private) | `sm_SbMonomials`, `sm_SbAlong`, `sm_SbOver` |
//! | `GX`, `GW` (private)         | `SM_SB_GX`, `SM_SB_GW`                     |
//! | `affine_solutions` (private) | `sm_SbAffineSolutions`                     |
//! | `least_squares` (private)    | `sm_SbLeastSquares`                        |
//!
//! Deviations: out-params are return values; a refusal's status and reason are one
//! [`Refusal`] (the fork's `Fill` is cleared on refusal, so no fill is returned then);
//! `SER`/`NER` of a call is that call's error, with no reason. The context argument of
//! `EaseSurface` and `FillHole` is dropped (they use it only to copy and build surfaces). A
//! round's surface is a [`BSplineSurface`] (the fork takes one and casts its copy back). The
//! orientation test of `sm_EaseSurface` evaluates the B-spline (#1376). Where the fork's
//! unsigned arithmetic would wrap into an out-of-bounds read (an index or row below what it
//! subtracts) the port returns `SM_ERR`, and a least-squares system with fewer rows than
//! columns (where the fork would allocate a wrapped size) is not solvable (`FALSE`); the
//! fork's cases never reach these. The moved ease's errors name this module as their
//! operation; their statuses are unchanged. No recursion.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // the fork's NaN-safe tests

use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::{Point2, Vec2};
use crate::geom::vector3d::sqrt;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, EFF_ZERO, PI};
use crate::nurbs::b_spline_curve::KnotType;
use crate::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData, SurfParamType,
};
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::knots::Side;
use crate::surface::surface::{Surface, SurfaceKind};

const OP: &str = "blend::blend_setback";

/// `SM_ERR`.
fn sm_err() -> SolidError {
    SolidError::sm_err(OP)
}

// ---- the ease of a round into another section before its setback ----------------------

/// `SmEaseRamp`: a stretch of a fillet surface's rail parameter over which its sections ease:
/// `w = 0` at `from` (the round's own section), `w = 1` at `to` (the curvature-continuous
/// one), by the C2 law `w(s) = 10 s^3 - 15 s^4 + 6 s^5` (its Bernstein coefficients
/// `0, 0, 0, 1, 1, 1`), which is flat to second order at both ends; `w` stays 1 beyond `to`.
#[derive(Debug, Clone, Copy)]
pub struct EaseRamp {
    /// Where `w = 0`: the round's own section (`m_dFrom`).
    pub from: f64,
    /// Where `w = 1`: the section eased into (`m_dTo`).
    pub to: f64,
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
pub(crate) fn interval_in(
    domain: &Extent2d,
    dir: SurfParamType,
) -> crate::geom::extent1d::Extent1d {
    if dir == SurfParamType::U {
        domain.u_interval()
    } else {
        domain.v_interval()
    }
}

/// The UV point at `along` in the rails' direction `dir` and `across` in the other.
pub(crate) fn uv_at(dir: SurfParamType, along: f64, across: f64) -> Point2 {
    if dir == SurfParamType::U {
        Point2::new(along, across)
    } else {
        Point2::new(across, along)
    }
}

/// `SmBlendSetback::EaseSurface` (the fork's `sm_EaseSurface`, moved here unchanged): the
/// surface whose sections ease from `from`'s to `to`'s over the ramps along `along`, the
/// rails' direction, in which both run over the same interval with the same rail parameter: `P = (1 - w) From + w To` in homogeneous coordinates, `w` the C2 law,
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
pub fn ease_surface(
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
    let (a, b) = ease_prologue(
        EaseSide::Net(Box::new(from.clone())),
        EaseSide::Net(Box::new(to.clone())),
        along,
        across,
    )?;
    ease_nets(a, b, along, across, ramps)
}

/// [`ease_surface`] of two fillet surfaces that may be the fork's procedural blends
/// (`SmProceduralBlend`, opt-in `fillet_round_as_definition`): the fork's `EaseSurface`
/// takes them as `SmBSplineSurface`s, and its copies' `Reparameterize`, `Reverse` and
/// `EvaluatePoint` are a blend's own (its map follows, its points are its
/// definition's); the ease itself is made of the nets. Surfaces that are not blends go
/// through [`ease_surface`] on their B-spline parts, as before.
///
/// # Errors
///
/// Those of [`ease_surface`]; `SM_ERR_NULL_POINTER` for a surface with no B-spline part.
pub fn ease_surface_of(
    k: &Kernel,
    from: &Surface,
    to: &Surface,
    along: SurfParamType,
    ramps: &[EaseRamp],
) -> Result<BSplineSurface> {
    let part = |s: &Surface| -> Result<BSplineSurface> {
        Ok(crate::topology::brep::p3::with_nurb(k, s)?
            .ok_or_else(|| SolidError::null_pointer(OP))?
            .into_owned())
    };
    let blend = |s: &Surface| matches!(s.kind(), SurfaceKind::ProceduralBlend(_));
    if !blend(from) && !blend(to) {
        return ease_surface(&part(from)?, &part(to)?, along, ramps);
    }
    let across = if along == SurfParamType::U {
        SurfParamType::V
    } else {
        SurfParamType::U
    };
    // (crFrom.Copy, crTo.Copy: a blend's copy keeps its definition)
    let side = |s: &Surface| -> Result<EaseSide<'_>> {
        Ok(if blend(s) {
            EaseSide::Blend(k, Box::new(s.copy(k)?))
        } else {
            EaseSide::Net(Box::new(part(s)?))
        })
    };
    let (a, b) = ease_prologue(side(from)?, side(to)?, along, across)?;
    ease_nets(a, b, along, across, ramps)
}

/// A surface `EaseSurface` orients: a B-spline, or a procedural blend whose
/// reparametrization, reversal and points are its own.
enum EaseSide<'k> {
    /// An `SmBSplineSurface` (or the B-spline part of an analytic one).
    Net(Box<BSplineSurface>),
    /// The fork's `SmProceduralBlend`.
    Blend(&'k Kernel, Box<Surface>),
}

impl EaseSide<'_> {
    fn natural_uv_domain(&self) -> Result<Extent2d> {
        match self {
            Self::Net(b) => Ok(b.natural_uv_domain()),
            Self::Blend(k, s) => s.natural_uv_domain_with_nurb(k),
        }
    }

    fn reparameterize(&mut self, domain: &Extent2d) -> Result<()> {
        match self {
            Self::Net(b) => b.reparameterize(domain),
            Self::Blend(k, s) => s.reparameterize(k, domain),
        }
    }

    fn reverse(&mut self, dir: SurfParamType) -> Result<()> {
        match self {
            Self::Net(b) => b.reverse(dir),
            Self::Blend(k, s) => s.reverse(k, dir),
        }
    }

    fn evaluate_point(&self, uv: Point2) -> Result<Point3> {
        match self {
            Self::Net(b) => b.evaluate_point(uv),
            Self::Blend(k, s) => s.evaluate_point(k, uv),
        }
    }

    /// The net the ease is made of.
    fn into_net(self) -> Result<BSplineSurface> {
        match self {
            Self::Net(b) => Ok(*b),
            Self::Blend(_, s) => match s.into_kind() {
                SurfaceKind::ProceduralBlend(pb) => Ok(pb.net),
                _ => Err(SolidError::null_pointer(OP)),
            },
        }
    }
}

/// `EaseSurface`'s first block: `to` on `from`'s domain, turned across when it runs the
/// other way; returns the two nets.
fn ease_prologue(
    a: EaseSide<'_>,
    mut b: EaseSide<'_>,
    along: SurfParamType,
    across: SurfParamType,
) -> Result<(BSplineSurface, BSplineSurface)> {
    // the same domain: along, the rail parameter in both; across, crFrom's
    let dom_a = a.natural_uv_domain()?;
    let dom_b = b.natural_uv_domain()?;
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

    Ok((a.into_net()?, b.into_net()?))
}

/// The rest of `EaseSurface`, on the two nets.
fn ease_nets(
    mut a: BSplineSurface,
    mut b: BSplineSurface,
    along: SurfParamType,
    across: SurfParamType,
    ramps: &[EaseRamp],
) -> Result<BSplineSurface> {
    // (the prologue's, from `a`, which it does not change)
    let along_a = interval_in(&a.natural_uv_domain(), along);
    let param_tol = 1.0e-9 * (1.0 + along_a.length());

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

// ---- the setback distances ------------------------------------------------------------

/// `smos_Max`: the larger, `b` when they compare false (NaN).
fn smax(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// `smos_Min`: the smaller, `b` when they compare false (NaN).
fn smin(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

/// An element of a slice, `SM_ERR` past its end.
fn el<T: Copy>(v: &[T], i: usize) -> Result<T> {
    v.get(i).copied().ok_or_else(sm_err)
}

/// A reference to an element of a slice, `SM_ERR` past its end.
fn el_ref<T>(v: &[T], i: usize) -> Result<&T> {
    v.get(i).ok_or_else(sm_err)
}

/// A mutable reference to an element of a slice, `SM_ERR` past its end.
fn el_mut<T>(v: &mut [T], i: usize) -> Result<&mut T> {
    v.get_mut(i).ok_or_else(sm_err)
}

/// Why the setback corner refused, or the failure of a call it made (`SM_SB_FAIL`: the
/// status the fork returns and the reason it leaves in `Fill::m_pReason` or `*ppReason`).
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    /// The status: `SM_ERR_INVALID_INPUT` ([`SolidError::InvalidInput`], with the reason) or
    /// `SM_ERR` for a refusal; that of the call for a failure of one (`SER`, `NER`).
    pub error: SolidError,
    /// The refusal's reason; `None` for the failure of a call (the fork leaves it `NULL`).
    pub reason: Option<&'static str>,
}

impl From<SolidError> for Refusal {
    fn from(error: SolidError) -> Self {
        Self {
            error,
            reason: None,
        }
    }
}

impl Refusal {
    /// `SM_SB_FAIL(.., SM_ERR_INVALID_INPUT, reason)`.
    fn invalid(reason: &'static str) -> Self {
        Self {
            error: SolidError::invalid_input(OP, reason),
            reason: Some(reason),
        }
    }

    /// `SM_SB_FAIL(.., SM_ERR, reason)`.
    fn sm_err(reason: &'static str) -> Self {
        Self {
            error: sm_err(),
            reason: Some(reason),
        }
    }
}

/// `SmBlendSetback::Face`: a face at the corner, a plane through `point`, `normal` its unit
/// normal out of the material.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Face {
    /// A point of the plane (`m_sPoint`).
    pub point: Point3,
    /// Its unit normal out of the material (`m_sNormal`).
    pub normal: Vec3,
}

/// `SmBlendSetback::Edge`: a rounded edge at the corner, for the setbacks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Edge {
    /// Its unit direction away from the vertex (`m_sAway`).
    pub away: Vec3,
    /// The round's radius there (`m_dRadius`).
    pub radius: f64,
    /// Its two faces, indices into the faces (`m_alFaces`).
    pub faces: [usize; 2],
}

/// What [`distances`] works out, one per edge in the edges' order.
#[derive(Debug, Clone, PartialEq)]
pub struct Setbacks {
    /// Each edge's setback (`rvSetbacks`).
    pub setbacks: Vec<f64>,
    /// Each edge's least setback (`*pvLeast`).
    pub least: Vec<f64>,
}

/// `SmBlendSetback::Distances`: each edge's setback, the owner's rule: the larger of `1.5 r`
/// and its least setback plus `0.25 r`. Its least setback is the farthest along it, over its
/// two faces, that its rail crosses the rail of the other edge on that face: a round between
/// planes whose normals are `psi` apart has its rails `r tan(psi / 2)` from its edge, and on a
/// face where edges i, j meet at the angle `alpha`, rails `d_i`, `d_j` from them cross
/// `(d_j + d_i cos alpha) / sin alpha` along edge i.
///
/// # Errors
/// [`Refusal`] with `SM_ERR_INVALID_INPUT` and its reason: an edge whose faces are not two of
/// the faces given or whose radius is not positive and finite, a face not shared by exactly
/// two of the edges, two edges along one line on a face.
pub fn distances(faces: &[Face], edges: &[Edge]) -> core::result::Result<Setbacks, Refusal> {
    let lf = faces.len();
    let le = edges.len();
    // each edge's rail distance from it, the same on both its faces (a round between planes)
    let mut rail = vec![0.0; le];
    for (i, e) in edges.iter().enumerate() {
        let [f0, f1] = e.faces;
        if !(e.radius > 0.0) || !e.radius.is_finite() || f0 >= lf || f1 >= lf || f0 == f1 {
            return Err(Refusal::invalid(
                "SmBlendSetback: an edge needs a positive radius and two of the faces given",
            ));
        }
        let cos = smax(
            -1.0,
            smin(
                1.0,
                el_ref(faces, f0)?.normal.dot(el_ref(faces, f1)?.normal),
            ),
        );
        *el_mut(&mut rail, i)? = e.radius * (0.5 * cos.acos()).tan();
    }
    let mut least = vec![0.0; le];
    for f in 0..lf {
        let beside: Vec<usize> = edges
            .iter()
            .enumerate()
            .filter(|(_, e)| e.faces[0] == f || e.faces[1] == f)
            .map(|(i, _)| i)
            .collect();
        let (Some(&i), Some(&j), 2) = (beside.first(), beside.get(1), beside.len()) else {
            return Err(Refusal::invalid(
                "SmBlendSetback: a face at the corner must lie between exactly two of its rounded edges",
            ));
        };
        let cos = smax(
            -1.0,
            smin(1.0, el_ref(edges, i)?.away.dot(el_ref(edges, j)?.away)),
        );
        let sin = sqrt(smax(0.0, 1.0 - cos * cos));
        if !(sin > 1.0e-9) {
            return Err(Refusal::invalid(
                "SmBlendSetback: two edges on a face run along one line",
            ));
        }
        let (rail_i, rail_j) = (el(&rail, i)?, el(&rail, j)?);
        let least_i = el_mut(&mut least, i)?;
        *least_i = smax(*least_i, (rail_j + rail_i * cos) / sin);
        let least_j = el_mut(&mut least, j)?;
        *least_j = smax(*least_j, (rail_i + rail_j * cos) / sin);
    }
    let mut setbacks = Vec::with_capacity(le);
    for (e, &l) in edges.iter().zip(&least) {
        let r = e.radius;
        setbacks.push(smax(1.5 * r, l + 0.25 * r));
    }
    Ok(Setbacks { setbacks, least })
}

// ---- the hole's fill ------------------------------------------------------------------

/// `sm_SbH`: a point in homogeneous coordinates `(x w, y w, z w, w)`.
type Hom = [f64; 4];

/// `sm_SbZero`.
const HOM_ZERO: Hom = [0.0; 4];

/// `sm_SbAxpy`: `y + s x`.
fn axpy(y: &Hom, s: f64, x: &Hom) -> Hom {
    let mut h = *y;
    for (hc, xc) in h.iter_mut().zip(x) {
        *hc += s * xc;
    }
    h
}

/// The Euclidean point of a homogeneous one.
fn euclid(h: &Hom) -> Point3 {
    Point3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3])
}

/// `sm_SbB`: Bernstein coefficients of a polynomial on `[0, 1]` (degree `len - 1`).
type Bern = Vec<f64>;

/// `sm_SbMul`: the product of two Bernstein polynomials.
fn bern_mul(a: &[f64], b: &[f64]) -> Result<Bern> {
    let p = a.len().checked_sub(1).ok_or_else(sm_err)?;
    let q = b.len().checked_sub(1).ok_or_else(sm_err)?;
    let mut c = vec![0.0; p + q + 1];
    for (i, &ai) in a.iter().enumerate() {
        for (j, &bj) in b.iter().enumerate() {
            let f = binomial(p, i)? * binomial(q, j)? / binomial(p + q, i + j)?;
            *el_mut(&mut c, i + j)? += f * ai * bj;
        }
    }
    Ok(c)
}

/// `sm_SbElevate`: raised to degree `deg` (at least its own).
fn bern_elevate(a: &[f64], deg: usize) -> Result<Bern> {
    let mut a = a.to_vec();
    loop {
        let p = a.len().checked_sub(1).ok_or_else(sm_err)?;
        if p >= deg {
            return Ok(a);
        }
        let mut b = vec![0.0; p + 2];
        *el_mut(&mut b, 0)? = el(&a, 0)?;
        *el_mut(&mut b, p + 1)? = el(&a, p)?;
        for i in 1..=p {
            *el_mut(&mut b, i)? = (i as f64 / (p + 1) as f64) * el(&a, i - 1)?
                + (1.0 - i as f64 / (p + 1) as f64) * el(&a, i)?;
        }
        a = b;
    }
}

/// `sm_SbT`: tensor-product Bernstein coefficients, degree `(p, q)`, `c[i (q + 1) + j]`.
#[derive(Debug, Clone, Default)]
struct Tensor {
    p: usize,
    q: usize,
    c: Vec<f64>,
}

/// `sm_SbTMul`: the product of two tensor-product Bernstein polynomials.
fn tensor_mul(a: &Tensor, b: &Tensor) -> Result<Tensor> {
    let (cp, cq) = (a.p + b.p, a.q + b.q);
    let mut c = vec![0.0; (cp + 1) * (cq + 1)];
    for i in 0..=a.p {
        for j in 0..=a.q {
            for k in 0..=b.p {
                for l in 0..=b.q {
                    let f = binomial(a.p, i)? * binomial(b.p, k)? / binomial(cp, i + k)?
                        * binomial(a.q, j)?
                        * binomial(b.q, l)?
                        / binomial(cq, j + l)?;
                    let x = f * el(&a.c, i * (a.q + 1) + j)? * el(&b.c, k * (b.q + 1) + l)?;
                    *el_mut(&mut c, (i + k) * (cq + 1) + (j + l))? += x;
                }
            }
        }
    }
    Ok(Tensor { p: cp, q: cq, c })
}

/// `sm_SbTElevate`: raised to degree `(lp, lq)`.
fn tensor_elevate(a: &Tensor, lp: usize, lq: usize) -> Result<Tensor> {
    let mut sa = vec![0.0; (lp + 1) * (a.q + 1)];
    for j in 0..=a.q {
        let mut col = vec![0.0; a.p + 1];
        for (i, ci) in col.iter_mut().enumerate() {
            *ci = el(&a.c, i * (a.q + 1) + j)?;
        }
        let col = bern_elevate(&col, lp)?;
        for i in 0..=lp {
            *el_mut(&mut sa, i * (a.q + 1) + j)? = el(&col, i)?;
        }
    }
    let mut c = vec![0.0; (lp + 1) * (lq + 1)];
    for i in 0..=lp {
        let row = sa
            .get(i * (a.q + 1)..(i + 1) * (a.q + 1))
            .ok_or_else(sm_err)?;
        let row = bern_elevate(row, lq)?;
        for j in 0..=lq {
            *el_mut(&mut c, i * (lq + 1) + j)? = el(&row, j)?;
        }
    }
    Ok(Tensor { p: lp, q: lq, c })
}

/// `sm_SbBasis`: the Bernstein basis of degree `p` at `t`, and its first and second
/// derivatives.
fn basis(p: usize, t: f64) -> Result<(Vec<f64>, Vec<f64>, Vec<f64>)> {
    // the basis of every degree up to p, by the recurrence
    let mut all: Vec<Vec<f64>> = Vec::with_capacity(p + 1);
    all.push(vec![1.0]);
    for d in 1..=p {
        let prev = el_ref(&all, d - 1)?;
        let mut row = vec![0.0; d + 1];
        for (i, ri) in row.iter_mut().enumerate() {
            let left = if i > 0 { el(prev, i - 1)? } else { 0.0 };
            let here = if i < d { el(prev, i)? } else { 0.0 };
            *ri = t * left + (1.0 - t) * here;
        }
        all.push(row);
    }
    let b0 = el_ref(&all, p)?.clone();
    let mut b1 = vec![0.0; p + 1];
    let mut b2 = vec![0.0; p + 1];
    let dp = p as f64;
    if p >= 1 {
        let prev = el_ref(&all, p - 1)?;
        for (i, b) in b1.iter_mut().enumerate() {
            let left = if i > 0 { el(prev, i - 1)? } else { 0.0 };
            let here = if i < p { el(prev, i)? } else { 0.0 };
            *b = dp * (left - here);
        }
    }
    if p >= 2 {
        let prev = el_ref(&all, p - 2)?;
        for (i, b) in b2.iter_mut().enumerate() {
            let d2 = if i >= 2 { el(prev, i - 2)? } else { 0.0 };
            let d1 = if i >= 1 && i - 1 <= p - 2 {
                el(prev, i - 1)?
            } else {
                0.0
            };
            let d0 = if i <= p - 2 { el(prev, i)? } else { 0.0 };
            *b = dp * (dp - 1.0) * (d2 - 2.0 * d1 + d0);
        }
    }
    Ok((b0, b1, b2))
}

/// `sm_SbNet`: a homogeneous tensor-product Bezier net, `(p + 1) x (q + 1)` points,
/// `pts[i (q + 1) + j]`, i along u.
#[derive(Debug, Clone, Default)]
struct Net {
    p: usize,
    q: usize,
    pts: Vec<Hom>,
}

/// The first three components of a homogeneous point, as a vector.
fn xyz(h: &Hom) -> Vec3 {
    Vec3::new(h[0], h[1], h[2])
}

/// `sm_SbSecond`: the second derivatives `R_uu`, `R_uv`, `R_vv` (Euclidean) at `(u, v)`.
fn second(net: &Net, u: f64, v: f64) -> Result<[Vec3; 3]> {
    let (u0, u1, u2) = basis(net.p, u)?;
    let (v0, v1, v2) = basis(net.q, v)?;
    // homogeneous derivatives: 0 value, 1 u, 2 v, 3 uu, 4 uv, 5 vv
    let mut h = [HOM_ZERO; 6];
    for i in 0..=net.p {
        for j in 0..=net.q {
            let a = el_ref(&net.pts, i * (net.q + 1) + j)?;
            let (ui0, ui1, ui2) = (el(&u0, i)?, el(&u1, i)?, el(&u2, i)?);
            let (vj0, vj1, vj2) = (el(&v0, j)?, el(&v1, j)?, el(&v2, j)?);
            let b = [
                ui0 * vj0,
                ui1 * vj0,
                ui0 * vj1,
                ui2 * vj0,
                ui1 * vj1,
                ui0 * vj2,
            ];
            for (hk, bk) in h.iter_mut().zip(b) {
                for (hkc, ac) in hk.iter_mut().zip(a) {
                    *hkc += bk * ac;
                }
            }
        }
    }
    let [h0, h1, h2, h3, h4, h5] = h;
    let w = h0[3];
    let r = xyz(&h0) / w;
    let ru = (xyz(&h1) - r * h1[3]) / w;
    let rv = (xyz(&h2) - r * h2[3]) / w;
    Ok([
        (xyz(&h3) - ru * 2.0 * h1[3] - r * h3[3]) / w,
        (xyz(&h4) - ru * h2[3] - rv * h1[3] - r * h4[3]) / w,
        (xyz(&h5) - rv * 2.0 * h2[3] - r * h5[3]) / w,
    ])
}

/// `sm_SbFirst`: the point and its first derivatives (Euclidean) at `(u, v)`.
fn first(net: &Net, u: f64, v: f64) -> Result<(Vec3, Vec3, Vec3)> {
    let (u0, u1, _) = basis(net.p, u)?;
    let (v0, v1, _) = basis(net.q, v)?;
    let mut h = [HOM_ZERO; 3];
    for i in 0..=net.p {
        for j in 0..=net.q {
            let a = el_ref(&net.pts, i * (net.q + 1) + j)?;
            let (ui0, ui1, vj0, vj1) = (el(&u0, i)?, el(&u1, i)?, el(&v0, j)?, el(&v1, j)?);
            let [ha, hb, hc] = &mut h;
            for (c, &ac) in a.iter().enumerate() {
                *el_mut(ha, c)? += ui0 * vj0 * ac;
                *el_mut(hb, c)? += ui1 * vj0 * ac;
                *el_mut(hc, c)? += ui0 * vj1 * ac;
            }
        }
    }
    let [h0, h1, h2] = h;
    let w = h0[3];
    let p = xyz(&h0) / w;
    Ok((p, (xyz(&h1) - p * h1[3]) / w, (xyz(&h2) - p * h2[3]) / w))
}

/// A round's homogeneous jets at its end section (`sm_SbRoundJets`'s out-params).
struct RoundJets {
    /// `H[j]` (`j = 0 ..= k`): the across Bezier curve of the j-th derivative by the rail
    /// parameter, columns from its across start.
    h: Vec<Vec<Hom>>,
    /// Its across degree.
    q: usize,
}

/// `sm_SbRoundJets`: a round's homogeneous derivatives along its rails at its end section,
/// to order `k`, and its across degree.
fn round_jets(round: &Round<'_>, k: usize) -> core::result::Result<RoundJets, Refusal> {
    let along = round.along;
    if along != SurfParamType::U && along != SurfParamType::V {
        return Err(Refusal::invalid(
            "SmBlendSetback: a round's rails run along u or v",
        ));
    }
    let across = if along == SurfParamType::U {
        SurfParamType::V
    } else {
        SurfParamType::U
    };
    let mut s = round.surface.clone();
    let p = s.degree(along);
    let dom = s.natural_uv_domain();
    let along_ivl = interval_in(&dom, along);
    let end = round.end;
    let param_tol = 1.0e-12 * (1.0 + along_ivl.length() + end.abs());
    // the round lies below its end when the corner lies above it
    let inside = if round.corner_above {
        end > along_ivl.min() + param_tol && end <= along_ivl.max() + param_tol
    } else {
        end >= along_ivl.min() - param_tol && end < along_ivl.max() - param_tol
    };
    if !inside {
        return Err(Refusal::invalid(
            "SmBlendSetback: a round's end section lies outside its rails' interval",
        ));
    }
    raise_knot(&mut s, along, end, p, param_tol)?;

    let find_end = |knots: &[f64]| knots.iter().position(|&x| !((x - end).abs() > param_tol));
    {
        let canonical = s.canonical()?;
        let knots = if along == SurfParamType::U {
            &canonical.u_knots
        } else {
            &canonical.v_knots
        };
        let ia = find_end(knots).ok_or_else(sm_err)?;
        // the span on the round's side, raised to Bezier form at its other end
        let ib = if round.corner_above {
            ia.checked_sub(1).ok_or_else(sm_err)?
        } else {
            ia + 1
        };
        let other = el(knots, ib)?;
        raise_knot(&mut s, along, other, p, param_tol)?;
    }
    let canonical = s.canonical()?;
    let (knots, mults, across_knots) = if along == SurfParamType::U {
        (
            &canonical.u_knots,
            &canonical.u_multiplicities,
            &canonical.v_knots,
        )
    } else {
        (
            &canonical.v_knots,
            &canonical.v_multiplicities,
            &canonical.u_knots,
        )
    };
    if across_knots.len() != 2 {
        return Err(Refusal::invalid(
            "SmBlendSetback: a round must be one Bezier span across its rails",
        ));
    }
    let ia = find_end(knots).ok_or_else(sm_err)?;
    let row_of = |knot: usize| -> Result<usize> {
        if knot == 0 {
            return Ok(0);
        }
        let s: usize = mults.get(..knot).ok_or_else(sm_err)?.iter().sum();
        s.checked_sub(1).ok_or_else(sm_err)
    };
    let lq = s.degree(across);
    let pts = &canonical.control_points;
    let weights = &canonical.weights;
    let along_count = pts.len() / (lq + 1);
    let at_net = |i: usize, j: usize| -> Result<Hom> {
        let idx = if along == SurfParamType::U {
            i * (lq + 1) + j
        } else {
            j * along_count + i
        };
        let w = if weights.is_empty() {
            1.0
        } else {
            el(weights, idx)?
        };
        let pt = el(pts, idx)?;
        Ok([pt.x * w, pt.y * w, pt.z * w, w])
    };
    let row = row_of(ia)?;
    let delta = if round.corner_above {
        el(knots, ia)? - el(knots, ia.checked_sub(1).ok_or_else(sm_err)?)?
    } else {
        el(knots, ia + 1)? - el(knots, ia)?
    };
    if !(delta > 0.0) {
        return Err(sm_err().into());
    }
    // the span's rows in increasing rail parameter, Q[0 ..= p]
    let first_row = if round.corner_above {
        row.checked_sub(p).ok_or_else(sm_err)?
    } else {
        row
    };
    let mut rows = vec![vec![HOM_ZERO; lq + 1]; p + 1];
    for (i, qi) in rows.iter_mut().enumerate() {
        for (j, qij) in qi.iter_mut().enumerate() {
            if first_row + i >= along_count {
                return Err(sm_err().into());
            }
            *qij = at_net(first_row + i, j)?;
        }
    }
    let mut h = vec![vec![HOM_ZERO; lq + 1]; k + 1];
    for j in 0..=lq {
        // derivatives at the span's end the round meets its end section at: forward
        // differences from that end, signed by the parameter
        let at_top = round.corner_above;
        for d in 0..=k {
            if d > p {
                continue;
            }
            let mut hd = HOM_ZERO;
            let mut fall = 1.0;
            for l in 0..d {
                fall *= (p - l) as f64 / delta;
            }
            for l in 0..=d {
                let sign = if (d - l) % 2 == 0 { 1.0 } else { -1.0 };
                let c = fall * binomial(d, l)? * sign;
                // at the top: sum_l (-1)^(d-l) C(d,l) Q[p - d + l]; at the bottom:
                // sum_l (-1)^(d-l) C(d,l) Q[l]
                let qi = if at_top { p - d + l } else { l };
                hd = axpy(&hd, c, el_ref(el_ref(&rows, qi)?, j)?);
            }
            *el_mut(el_mut(&mut h, d)?, j)? = hd;
        }
    }
    Ok(RoundJets { h, q: lq })
}

/// `sm_SbMonomials`: the monomials `x^a y^b`, `a + b <= d`, in a fixed order.
fn monomials(d: usize) -> Vec<(usize, usize)> {
    let mut m = Vec::new();
    for t in 0..=d {
        for a in (0..=t).rev() {
            m.push((a, t - a));
        }
    }
    m
}

/// `sm_SbAlong`: `x^a y^b` along the line `(x(v), y(v))` (Bernstein, degree 1 each), raised
/// to degree `deg`.
fn along_line(a: usize, b: usize, x: &[f64], y: &[f64], deg: usize) -> Result<Bern> {
    let mut s = vec![1.0];
    for _ in 0..a {
        s = bern_mul(&s, x)?;
    }
    for _ in 0..b {
        s = bern_mul(&s, y)?;
    }
    bern_elevate(&s, deg)
}

/// `sm_SbOver`: `x^a y^b` over the bilinear patch `(x(s, t), y(s, t))`, raised to degree
/// `(deg, deg)`.
fn over_patch(a: usize, b: usize, x: &Tensor, y: &Tensor, deg: usize) -> Result<Tensor> {
    let mut s = Tensor {
        p: 0,
        q: 0,
        c: vec![1.0],
    };
    for _ in 0..a {
        s = tensor_mul(&s, x)?;
    }
    for _ in 0..b {
        s = tensor_mul(&s, y)?;
    }
    tensor_elevate(&s, deg, deg)
}

/// `SM_SB_GX`: Gauss-Legendre, 8 points on `[0, 1]`.
const GX: [f64; 8] = [
    0.019855071751231856,
    0.10166676129318664,
    0.2372337950418355,
    0.4082826787521751,
    0.5917173212478249,
    0.7627662049581645,
    0.8983332387068134,
    0.9801449282487681,
];

/// `SM_SB_GW`: their weights.
const GW: [f64; 8] = [
    0.05061426814518813,
    0.11119051722668724,
    0.15685332293894363,
    0.18134189168918100,
    0.18134189168918100,
    0.15685332293894363,
    0.11119051722668724,
    0.05061426814518813,
];

/// The solutions of `A x = b` ([`affine_solutions`]): `x = x0 + Z y`, the columns of `Z`
/// (`n - rank` of them).
struct Affine {
    x0: Vec<f64>,
    z: Vec<Vec<f64>>,
}

/// `sm_SbAffineSolutions`: the rows of `A` (`m x n`, row-major) reduced by Gaussian
/// elimination with complete pivoting: the solutions of `A x = b`. `None` (`FALSE`) when the
/// equations are inconsistent beyond `tol` (rows scaled to unit largest entry).
fn affine_solutions(
    mut a: Vec<f64>,
    mut b: Vec<f64>,
    m: usize,
    n: usize,
    tol: f64,
) -> Result<Option<Affine>> {
    for i in 0..m {
        let mut max = 0.0;
        for j in 0..n {
            max = smax(max, el(&a, i * n + j)?.abs());
        }
        if max > 0.0 {
            for j in 0..n {
                *el_mut(&mut a, i * n + j)? /= max;
            }
            *el_mut(&mut b, i)? /= max;
        }
    }
    let mut col: Vec<usize> = (0..n).collect();
    let mut r = 0;
    while r < m && r < n {
        let (mut pi, mut pj) = (r, r);
        let mut best = 0.0;
        for i in r..m {
            for j in r..n {
                let x = el(&a, i * n + j)?.abs();
                if x > best {
                    best = x;
                    pi = i;
                    pj = j;
                }
            }
        }
        if !(best > 1.0e-10) {
            break;
        }
        if pi != r {
            for j in 0..n {
                if a.get(pi * n + j).is_none() || a.get(r * n + j).is_none() {
                    return Err(sm_err());
                }
                a.swap(pi * n + j, r * n + j);
            }
            if b.get(pi).is_none() || b.get(r).is_none() {
                return Err(sm_err());
            }
            b.swap(pi, r);
        }
        if pj != r {
            for i in 0..m {
                if a.get(i * n + pj).is_none() || a.get(i * n + r).is_none() {
                    return Err(sm_err());
                }
                a.swap(i * n + pj, i * n + r);
            }
            if col.get(pj).is_none() || col.get(r).is_none() {
                return Err(sm_err());
            }
            col.swap(pj, r);
        }
        for i in 0..m {
            if i == r {
                continue;
            }
            let f = el(&a, i * n + r)? / el(&a, r * n + r)?;
            if f == 0.0 {
                continue;
            }
            for j in r..n {
                let arj = el(&a, r * n + j)?;
                *el_mut(&mut a, i * n + j)? -= f * arj;
            }
            let br = el(&b, r)?;
            *el_mut(&mut b, i)? -= f * br;
        }
        r += 1;
    }
    for i in r..m {
        if el(&b, i)?.abs() > tol {
            return Ok(None);
        }
    }
    // pivot rows: A[i][i] x_i + sum_{free j} A[i][j] x_j = b_i
    let mut x0 = vec![0.0; n];
    for i in 0..r {
        *el_mut(&mut x0, el(&col, i)?)? = el(&b, i)? / el(&a, i * n + i)?;
    }
    let mut z = Vec::new();
    for f in r..n {
        let mut zf = vec![0.0; n];
        *el_mut(&mut zf, el(&col, f)?)? = 1.0;
        for i in 0..r {
            *el_mut(&mut zf, el(&col, i)?)? = -el(&a, i * n + f)? / el(&a, i * n + i)?;
        }
        z.push(zf);
    }
    Ok(Some(Affine { x0, z }))
}

/// `sm_SbLeastSquares`: the least squares solution of `M y = c` (`rows x cols`, row-major,
/// `rows >= cols`) by Householder QR; `None` (`FALSE`) when `M`'s columns are dependent
/// (relative to `tol`), and for fewer rows than columns.
fn least_squares(
    mut mm: Vec<f64>,
    mut c: Vec<f64>,
    rows: usize,
    cols: usize,
    tol: f64,
) -> Result<Option<Vec<f64>>> {
    if rows < cols {
        return Ok(None);
    }
    let mut diag = vec![0.0; cols];
    for k in 0..cols {
        let mut norm = 0.0;
        for i in k..rows {
            let x = el(&mm, i * cols + k)?;
            norm += x * x;
        }
        let norm = sqrt(norm);
        if !(norm > 0.0) {
            return Ok(None);
        }
        let alpha = if el(&mm, k * cols + k)? > 0.0 {
            -norm
        } else {
            norm
        };
        let mut v = vec![0.0; rows - k];
        for i in k..rows {
            *el_mut(&mut v, i - k)? = el(&mm, i * cols + k)?;
        }
        *el_mut(&mut v, 0)? -= alpha;
        let mut dv = 0.0;
        for &vi in &v {
            dv += vi * vi;
        }
        if dv > 0.0 {
            for j in k..cols {
                let mut s = 0.0;
                for i in k..rows {
                    s += el(&v, i - k)? * el(&mm, i * cols + j)?;
                }
                let s = 2.0 * s / dv;
                for i in k..rows {
                    let vi = el(&v, i - k)?;
                    *el_mut(&mut mm, i * cols + j)? -= s * vi;
                }
            }
            let mut s = 0.0;
            for i in k..rows {
                s += el(&v, i - k)? * el(&c, i)?;
            }
            let s = 2.0 * s / dv;
            for i in k..rows {
                let vi = el(&v, i - k)?;
                *el_mut(&mut c, i)? -= s * vi;
            }
        }
        *el_mut(&mut diag, k)? = el(&mm, k * cols + k)?;
    }
    let mut max = 0.0;
    for &d in &diag {
        max = smax(max, d.abs());
    }
    for &d in &diag {
        if !(d.abs() > tol * max) {
            return Ok(None);
        }
    }
    let mut y = vec![0.0; cols];
    for k in (0..cols).rev() {
        let mut s = el(&c, k)?;
        for j in k + 1..cols {
            s -= el(&mm, k * cols + j)? * el(&y, j)?;
        }
        *el_mut(&mut y, k)? = s / el(&mm, k * cols + k)?;
    }
    Ok(Some(y))
}

/// `SmBlendSetback::Round`: a round at the corner, ending at its setback. The round must
/// meet its faces to the fill's order at its end section (a G1 round eased first for a G2
/// fill).
#[derive(Debug, Clone, Copy)]
pub struct Round<'a> {
    /// Its surface, one Bezier span across its rails, any along them (`m_pSurface`).
    pub surface: &'a BSplineSurface,
    /// Its rails' direction (`m_eAlong`).
    pub along: SurfParamType,
    /// The rail parameter of its end section (`m_dEnd`).
    pub end: f64,
    /// Whether the corner lies at rail parameters above `end`, the round below
    /// (`m_bCornerAbove`).
    pub corner_above: bool,
}

/// `SmBlendSetback::Fill`: the patches of a filled hole, in the cycle's order: rounds
/// `m = 0 .. n - 1` round the vertex, round m between faces m and `m + 1` (mod n).
///
/// `patches[m]`, `m < n`, round m's setback patch, `(u, v)` in `[0, 1]^2`:
/// - `u = 0`: round m's end section, v running from face m to face `m + 1`
///   (`reversed[m]`: its across parameter runs the other way);
/// - `v = 0`: on face m (a spring curve's half), `v = 1`: on face `m + 1`;
/// - `u = 1`: the inner curve from face m's base point (`v = 0`) to face `m + 1`'s
///   (`v = 1`), its half `v <= 1/2` interior part m's side `s = 1` (`t = 1 - 2 v`), its half
///   `v >= 1/2` interior part `m + 1`'s side `t = 1` (`s = 2 v - 1`).
///
/// `patches[n + q]`, interior part q, `(s, t)` in `[0, 1]^2`: `(0, 0)` the centre, `(1, 1)`
/// face q's base point; `t = 0` the spoke to the middle of inner curve q, which interior part
/// `q + 1` has as its side `s = 0` (its t the same as part q's s).
#[derive(Debug, Clone, Default)]
pub struct Fill {
    /// The setback patches, then the interior parts (`m_vPatches`).
    pub patches: Vec<BSplineSurface>,
    /// The caller's index of the cycle's round m (`m_vRound`).
    pub round: Vec<usize>,
    /// The caller's index of face m (`m_vFace`).
    pub face: Vec<usize>,
    /// Whether round m's across parameter runs from face `m + 1` to face m (`m_vReversed`).
    pub reversed: Vec<bool>,
    /// The interior map's degree (`m_lDegree`).
    pub degree: usize,
}

/// `SmBlendSetback::FillHole`: the hole the rounds leave at a corner of the faces, filled to
/// `order` (1: G1, 2: G2) by the setback split (module docs). `tol`: the length the faces and
/// the rounds' ends are matched to (the rounds' rails on their faces, their end rows' first
/// columns on the faces' planes). The fill is not held to the faces' half-spaces here:
/// [`containment`] bounds that.
///
/// The cycle: round m between faces m (its setback patch's `v = 0`) and `m + 1`. The domain:
/// the regular n-gon with vertices `B_m = (cos 2 pi m / n, sin 2 pi m / n)`, `B_m` the base
/// point of face m; its edge m from `B_m` to `B_m+1` is the inner curve of setback patch m; at
/// `B_m` the direction field of edge m starts as `sigma_m = t_m + t_m-1` (t the edges' unit
/// directions) and that of edge `m - 1` ends as `-sigma_m`, so the two setback patches leave
/// face m's base point along one line (the spring curve is tangent there).
///
/// # Errors
/// [`Refusal`] with `SM_ERR_INVALID_INPUT` and its reason: rounds that do not close round the
/// vertex face to face, a round with more than one span across, a round that does not meet
/// its faces to `order` at its end, an order other than 1 or 2. With `SM_ERR` and its reason:
/// conditions on the interior map no degree up to 9 can meet, or a fill that folds. A failing
/// call's error, without a reason.
pub fn fill_hole(
    faces: &[Face],
    rounds: &[Round<'_>],
    order: usize,
    tol: f64,
) -> core::result::Result<Fill, Refusal> {
    if order != 1 && order != 2 {
        return Err(Refusal::invalid(
            "SmBlendSetback: a setback fill is of order 1 (G1) or 2 (G2)",
        ));
    }
    if !(tol > 0.0) || rounds.len() < 3 || faces.len() != rounds.len() {
        return Err(Refusal::invalid(
            "SmBlendSetback: a setback fill needs three or more rounds, as many faces, and a positive tolerance",
        ));
    }
    let n = rounds.len();
    let k = order;
    let big_n = 2 * k + 1; // the setback patches' degree across the hole

    // each round's jets at its end, and the faces its across ends lie on
    let mut jets: Vec<Vec<Vec<Hom>>> = Vec::with_capacity(n);
    let mut qs: Vec<usize> = Vec::with_capacity(n);
    let mut face_at = [vec![n; n], vec![n; n]];
    for (r, round) in rounds.iter().enumerate() {
        let RoundJets { h, q } = round_jets(round, k)?;
        for (e, at_e) in face_at.iter_mut().enumerate() {
            let hp = el_ref(el_ref(&h, 0)?, if e == 0 { 0 } else { q })?;
            let p = euclid(hp);
            let slot = el_mut(at_e, r)?;
            for (f, face) in faces.iter().enumerate() {
                if (p - face.point).dot(face.normal).abs() <= tol {
                    if *slot != n {
                        return Err(Refusal::invalid(
                            "SmBlendSetback: a round's rail lies on two of the faces",
                        ));
                    }
                    *slot = f;
                }
            }
            if *slot == n {
                return Err(Refusal::invalid(
                    "SmBlendSetback: a round's rail lies on none of the faces",
                ));
            }
        }
        jets.push(h);
        qs.push(q);
    }
    // the cycle, face to face
    let mut cycle_round: Vec<usize> = Vec::with_capacity(n);
    let mut cycle_face: Vec<usize> = Vec::with_capacity(n);
    let mut cycle_rev: Vec<bool> = Vec::with_capacity(n);
    {
        let not_closed = || {
            Refusal::invalid(
                "SmBlendSetback: the rounds do not close round the vertex face to face",
            )
        };
        let [at0, at1] = &face_at;
        let mut used = vec![false; n];
        let mut r = 0;
        let mut rev = false;
        cycle_face.push(el(at0, 0)?);
        for m in 0..n {
            *el_mut(&mut used, r)? = true;
            cycle_round.push(r);
            cycle_rev.push(rev);
            let next = if rev { el(at0, r)? } else { el(at1, r)? };
            if m + 1 == n {
                if next != el(&cycle_face, 0)? {
                    return Err(not_closed());
                }
                break;
            }
            cycle_face.push(next);
            let mut found = n;
            for o in 0..n {
                if !el(&used, o)? && (el(at0, o)? == next || el(at1, o)? == next) {
                    if found != n {
                        return Err(Refusal::invalid(
                            "SmBlendSetback: a face lies between more than two of the rounds",
                        ));
                    }
                    found = o;
                }
            }
            if found == n {
                return Err(not_closed());
            }
            rev = el(at1, found)? == next;
            r = found;
        }
        let mut face_used = vec![false; n];
        for &f in &cycle_face {
            let slot = el_mut(&mut face_used, f)?;
            if *slot {
                return Err(not_closed());
            }
            *slot = true;
        }
    }
    let face_of = |m: usize| -> Result<&Face> { el_ref(faces, el(&cycle_face, m % n)?) };

    // the domain
    let nf = n as f64;
    let bs: Vec<Point2> = (0..n)
        .map(|m| {
            Point2::new(
                (2.0 * PI * m as f64 / nf).cos(),
                (2.0 * PI * m as f64 / nf).sin(),
            )
        })
        .collect();
    let mut ts: Vec<Vec2> = Vec::with_capacity(n);
    for m in 0..n {
        ts.push((el(&bs, (m + 1) % n)? - el(&bs, m)?).unitize()?);
    }
    let mut sigma: Vec<Vec2> = Vec::with_capacity(n);
    for m in 0..n {
        sigma.push(el(&ts, m)? + el(&ts, (m + n - 1) % n)?);
    }

    // the setback patches' first k + 1 rows, from the rounds: homogeneous Hermite data at
    // u = 0, the rail parameter run toward the corner at a speed lambda giving the patch a
    // speed across of its spoke (from its end section's middle to the middle of the end
    // sections' middles). Half that left saddles in the box corner's setback patches
    // (Gaussian curvature down to -0.82 at r 2, against the corner ball's 0.25); the whole
    // spoke, -0.25
    let mut round_rows: Vec<Vec<Vec<Hom>>> = Vec::with_capacity(n); // [m][i][j], i = 0 ..= k
    {
        let mut mids: Vec<Point3> = Vec::with_capacity(n);
        let mut centre = Vec3::ZERO;
        let mut speeds: Vec<f64> = Vec::with_capacity(n);
        let flags = EvaluateFlags {
            u_side: Side::Left,
            v_side: Side::Left,
            only_upper_half: false,
            non_zero_tangents: false,
        };
        for &r in &cycle_round {
            let round = el_ref(rounds, r)?;
            let dom = round.surface.natural_uv_domain();
            let across = if round.along == SurfParamType::U {
                dom.v_interval()
            } else {
                dom.u_interval()
            };
            let uv = if round.along == SurfParamType::U {
                Point2::new(round.end, across.evaluate(0.5))
            } else {
                Point2::new(across.evaluate(0.5), round.end)
            };
            let d = round.surface.evaluate(uv, 1, 1, flags)?;
            let row0 = el_ref(&d, 0)?;
            let point = el(row0, 0)?;
            let speed = if round.along == SurfParamType::U {
                el(el_ref(&d, 1)?, 0)?
            } else {
                el(row0, 1)?
            };
            mids.push(point);
            speeds.push(speed.length());
            centre += point * (1.0 / nf);
        }
        for m in 0..n {
            let r = el(&cycle_round, m)?;
            let round = el_ref(rounds, r)?;
            let speed = el(&speeds, m)?;
            if !(speed > 0.0) {
                return Err(Refusal::invalid(
                    "SmBlendSetback: a round does not move along its rails at its end",
                ));
            }
            let mut lambda = (el(&mids, m)? - centre).length() / speed;
            if !round.corner_above {
                lambda = -lambda;
            }
            let q = el(&qs, r)?;
            let rev = el(&cycle_rev, m)?;
            let jets_r = el_ref(&jets, r)?;
            let mut hs = vec![vec![HOM_ZERO; q + 1]; k + 1];
            let mut scale = 1.0;
            for (j, hj) in hs.iter_mut().enumerate() {
                let jet = el_ref(jets_r, j)?;
                for (c, hjc) in hj.iter_mut().enumerate() {
                    let cc = if rev { q - c } else { c };
                    *hjc = el(jet, cc)?;
                    for x in hjc.iter_mut() {
                        *x *= scale;
                    }
                }
                scale *= lambda;
            }
            let mut rows = vec![vec![HOM_ZERO; q + 1]; k + 1];
            for c in 0..=q {
                let r0 = el(el_ref(&hs, 0)?, c)?;
                *el_mut(el_mut(&mut rows, 0)?, c)? = r0;
                if k >= 1 {
                    let r1 = axpy(&r0, 1.0 / big_n as f64, el_ref(el_ref(&hs, 1)?, c)?);
                    *el_mut(el_mut(&mut rows, 1)?, c)? = r1;
                    if k >= 2 {
                        let mut h = axpy(&HOM_ZERO, 2.0, &r1);
                        h = axpy(&h, -1.0, &r0);
                        let r2 = axpy(
                            &h,
                            1.0 / (big_n * (big_n - 1)) as f64,
                            el_ref(el_ref(&hs, 2)?, c)?,
                        );
                        *el_mut(el_mut(&mut rows, 2)?, c)? = r2;
                    }
                }
            }
            // they must meet the faces to order k: their first and last k + 1 columns on the
            // faces' planes
            for row in &rows {
                for c in 0..=k {
                    for e in 0..2 {
                        let col = if e == 0 {
                            c
                        } else {
                            q.checked_sub(c).ok_or_else(sm_err)?
                        };
                        let h = el_ref(row, col)?;
                        let face = face_of(m + e)?;
                        if !(h[3] > 0.0) {
                            return Err(Refusal::invalid(
                                "SmBlendSetback: a round's weights must be positive",
                            ));
                        }
                        if (euclid(h) - face.point).dot(face.normal).abs() > tol {
                            return Err(Refusal::invalid(
                                "SmBlendSetback: a round does not meet its faces to the fill's order at its end (a G1 round is eased first for a G2 fill)",
                            ));
                        }
                    }
                }
            }
            round_rows.push(rows);
        }
    }

    // the interior map's degree: the least from 2k + 1 at which its conditions can be met
    for d in big_n..=9 {
        let mono = monomials(d);
        let mc = mono.len();
        let unknowns = 3 * mc;

        // each edge's rows from F per monomial: D0, D1, D2 (degree d across)
        let mut edge_basis: Vec<Vec<Vec<Bern>>> = vec![vec![vec![Vec::new(); mc]; k + 1]; n];
        for (m, eb) in edge_basis.iter_mut().enumerate() {
            let b0 = el(&bs, m)?;
            let b1 = el(&bs, (m + 1) % n)?;
            let x = [b0.x, b1.x];
            let y = [b0.y, b1.y];
            let d0 = el(&sigma, m)?;
            let d1 = -el(&sigma, (m + 1) % n)?;
            let dx = [d0.x, d1.x];
            let dy = [d0.y, d1.y];
            let dxx = bern_mul(&dx, &dx)?;
            let dxy = bern_mul(&dx, &dy)?;
            let dyy = bern_mul(&dy, &dy)?;
            for (j, &(a, b)) in mono.iter().enumerate() {
                *el_mut(el_mut(eb, 0)?, j)? = along_line(a, b, &x, &y, d)?;
                // D1: x^a y^b differentiated along the field: a x^(a-1) y^b dx + b x^a
                // y^(b-1) dy
                let mut s1 = vec![0.0; d + 1];
                if a >= 1 {
                    let t =
                        bern_elevate(&bern_mul(&along_line(a - 1, b, &x, &y, d - 1)?, &dx)?, d)?;
                    for (si, &ti) in s1.iter_mut().zip(&t) {
                        *si += a as f64 * ti;
                    }
                }
                if b >= 1 {
                    let t =
                        bern_elevate(&bern_mul(&along_line(a, b - 1, &x, &y, d - 1)?, &dy)?, d)?;
                    for (si, &ti) in s1.iter_mut().zip(&t) {
                        *si += b as f64 * ti;
                    }
                }
                *el_mut(el_mut(eb, 1)?, j)? = s1;
                if k >= 2 {
                    let mut s2 = vec![0.0; d + 1];
                    if a >= 2 {
                        let t = bern_elevate(
                            &bern_mul(&along_line(a - 2, b, &x, &y, d - 2)?, &dxx)?,
                            d,
                        )?;
                        for (si, &ti) in s2.iter_mut().zip(&t) {
                            *si += (a * (a - 1)) as f64 * ti;
                        }
                    }
                    if a >= 1 && b >= 1 {
                        let t = bern_elevate(
                            &bern_mul(&along_line(a - 1, b - 1, &x, &y, d - 2)?, &dxy)?,
                            d,
                        )?;
                        for (si, &ti) in s2.iter_mut().zip(&t) {
                            *si += 2.0 * a as f64 * b as f64 * ti;
                        }
                    }
                    if b >= 2 {
                        let t = bern_elevate(
                            &bern_mul(&along_line(a, b - 2, &x, &y, d - 2)?, &dyy)?,
                            d,
                        )?;
                        for (si, &ti) in s2.iter_mut().zip(&t) {
                            *si += (b * (b - 1)) as f64 * ti;
                        }
                    }
                    *el_mut(el_mut(eb, 2)?, j)? = s2;
                }
            }
        }
        // each interior part's net per monomial, degree (d, d): corners (0, 0) the centre,
        // (1, 0) the middle of edge q, (1, 1) B_q, (0, 1) the middle of edge q - 1
        let mut part_basis: Vec<Vec<Tensor>> = Vec::with_capacity(n);
        for q in 0..n {
            let c = Point2::new(0.0, 0.0);
            let bq = el(&bs, q)?;
            let bn = el(&bs, (q + 1) % n)?;
            let bp = el(&bs, (q + n - 1) % n)?;
            let mq = Point2::new((bq.x + bn.x) * 0.5, (bq.y + bn.y) * 0.5);
            let mp = Point2::new((bq.x + bp.x) * 0.5, (bq.y + bp.y) * 0.5);
            let tx = Tensor {
                p: 1,
                q: 1,
                c: vec![c.x, mp.x, mq.x, bq.x],
            };
            let ty = Tensor {
                p: 1,
                q: 1,
                c: vec![c.y, mp.y, mq.y, bq.y],
            };
            let mut parts = Vec::with_capacity(mc);
            for &(a, b) in &mono {
                parts.push(over_patch(a, b, &tx, &ty, d)?);
            }
            part_basis.push(parts);
        }

        // the nets for a given vector of F's coefficients x[c M + j]
        let nets = |xs: &[f64]| -> Result<Vec<Net>> {
            let mut out: Vec<Net> = Vec::with_capacity(2 * n);
            for m in 0..n {
                let qv = el(&qs, el(&cycle_round, m)?)?.max(d);
                let mut net = Net {
                    p: big_n,
                    q: qv,
                    pts: vec![HOM_ZERO; (big_n + 1) * (qv + 1)],
                };
                // the round's rows, raised across to qv
                let rows_m = el_ref(&round_rows, m)?;
                for i in 0..=k {
                    let row = el_ref(rows_m, i)?;
                    for c in 0..4 {
                        let mut s: Bern = Vec::with_capacity(row.len());
                        for h in row {
                            s.push(el(h, c)?);
                        }
                        let s = bern_elevate(&s, qv)?;
                        for j in 0..=qv {
                            *el_mut(el_mut(&mut net.pts, i * (qv + 1) + j)?, c)? = el(&s, j)?;
                        }
                    }
                }
                // F's: E0 = D0, E1 = -D1, E2 = D2 at u = 1
                let eb = el_ref(&edge_basis, m)?;
                let mut es = vec![vec![HOM_ZERO; qv + 1]; k + 1];
                for (l, el_row) in es.iter_mut().enumerate() {
                    let ebl = el_ref(eb, l)?;
                    for c in 0..3 {
                        let mut s = vec![0.0; d + 1];
                        for j in 0..mc {
                            let x = el(xs, c * mc + j)?;
                            if x == 0.0 {
                                continue;
                            }
                            let ebj = el_ref(ebl, j)?;
                            for (i, si) in s.iter_mut().enumerate() {
                                *si += x * el(ebj, i)?;
                            }
                        }
                        let s = bern_elevate(&s, qv)?;
                        let sign = if l == 1 { -1.0 } else { 1.0 };
                        for (j, h) in el_row.iter_mut().enumerate() {
                            *el_mut(h, c)? = sign * el(&s, j)?;
                        }
                    }
                }
                for j in 0..=qv {
                    let mut pn = el(el_ref(&es, 0)?, j)?;
                    pn[3] = 1.0;
                    *el_mut(&mut net.pts, big_n * (qv + 1) + j)? = pn;
                    let pn1 = axpy(&pn, -1.0 / big_n as f64, el_ref(el_ref(&es, 1)?, j)?);
                    *el_mut(&mut net.pts, (big_n - 1) * (qv + 1) + j)? = pn1;
                    if k >= 2 {
                        let mut h = axpy(&HOM_ZERO, 2.0, &pn1);
                        h = axpy(&h, -1.0, &pn);
                        *el_mut(&mut net.pts, (big_n - 2) * (qv + 1) + j)? = axpy(
                            &h,
                            1.0 / (big_n * (big_n - 1)) as f64,
                            el_ref(el_ref(&es, 2)?, j)?,
                        );
                    }
                }
                out.push(net);
            }
            for parts in &part_basis {
                let mut net = Net {
                    p: d,
                    q: d,
                    pts: vec![[0.0, 0.0, 0.0, 1.0]; (d + 1) * (d + 1)],
                };
                for c in 0..3 {
                    for (j, part) in parts.iter().enumerate() {
                        let x = el(xs, c * mc + j)?;
                        if x == 0.0 {
                            continue;
                        }
                        for (h, &pc) in net.pts.iter_mut().zip(&part.c) {
                            *el_mut(h, c)? += x * pc;
                        }
                    }
                }
                out.push(net);
            }
            Ok(out)
        };
        // the conditions: F's rows of each setback patch, their first k + 1 columns on face
        // m's plane and their last k + 1 on face m + 1's
        let conditions = |nets: &[Net]| -> Result<Vec<f64>> {
            let mut out = Vec::new();
            for m in 0..n {
                let net = el_ref(nets, m)?;
                for i in big_n - k..=big_n {
                    for c in 0..=k {
                        for e in 0..2 {
                            let col = if e == 0 {
                                c
                            } else {
                                net.q.checked_sub(c).ok_or_else(sm_err)?
                            };
                            let h = el_ref(&net.pts, i * (net.q + 1) + col)?;
                            let face = face_of(m + e)?;
                            let nrm = face.normal;
                            out.push(
                                nrm.x * h[0] + nrm.y * h[1] + nrm.z * h[2]
                                    - nrm.dot(face.point) * h[3],
                            );
                        }
                    }
                }
            }
            Ok(out)
        };
        // the bending energy's terms, each squared and summed
        let energy = |nets: &[Net]| -> Result<Vec<f64>> {
            let mut out = Vec::new();
            for net in nets {
                for (&gx1, &gw1) in GX.iter().zip(&GW) {
                    for (&gx2, &gw2) in GX.iter().zip(&GW) {
                        let [ruu, ruv, rvv] = second(net, gx1, gx2)?;
                        let w = sqrt(gw1 * gw2);
                        for (uu, (uv, vv)) in [ruu.x, ruu.y, ruu.z]
                            .into_iter()
                            .zip([ruv.x, ruv.y, ruv.z].into_iter().zip([rvv.x, rvv.y, rvv.z]))
                        {
                            out.push(w * uu);
                            out.push(w * sqrt(2.0) * uv);
                            out.push(w * vv);
                        }
                    }
                }
            }
            Ok(out)
        };

        // both affine in x: their matrices column by column
        let x0 = vec![0.0; unknowns];
        let nets0 = nets(&x0)?;
        let c0 = conditions(&nets0)?;
        let e0 = energy(&nets0)?;
        let (lc, le) = (c0.len(), e0.len());
        let mut ac = vec![0.0; lc * unknowns];
        let mut ae = vec![0.0; le * unknowns];
        for u in 0..unknowns {
            let mut xs = vec![0.0; unknowns];
            *el_mut(&mut xs, u)? = 1.0;
            let nets_u = nets(&xs)?;
            let cu = conditions(&nets_u)?;
            let eu = energy(&nets_u)?;
            for i in 0..lc {
                *el_mut(&mut ac, i * unknowns + u)? = el(&cu, i)? - el(&c0, i)?;
            }
            for i in 0..le {
                *el_mut(&mut ae, i * unknowns + u)? = el(&eu, i)? - el(&e0, i)?;
            }
        }
        let mut rhs = vec![0.0; lc];
        let mut size = 0.0;
        for (ri, &ci) in rhs.iter_mut().zip(&c0) {
            *ri = -ci;
            size = smax(size, ci.abs());
        }
        let Some(Affine { x0: xp, z }) =
            affine_solutions(ac, rhs, lc, unknowns, 1.0e-9 * (1.0 + size))?
        else {
            continue;
        };
        if z.is_empty() {
            continue;
        }
        // the energy over x = xp + Z y
        let free = z.len();
        let mut mm = vec![0.0; le * free];
        let mut rr = vec![0.0; le];
        for i in 0..le {
            let mut s = el(&e0, i)?;
            for u in 0..unknowns {
                s += el(&ae, i * unknowns + u)? * el(&xp, u)?;
            }
            *el_mut(&mut rr, i)? = -s;
            for (f, zf) in z.iter().enumerate() {
                let mut t = 0.0;
                for u in 0..unknowns {
                    t += el(&ae, i * unknowns + u)? * el(zf, u)?;
                }
                *el_mut(&mut mm, i * free + f)? = t;
            }
        }
        let Some(y) = least_squares(mm, rr, le, free, 1.0e-12)? else {
            continue;
        };
        let mut xs = xp;
        for (zf, &yf) in z.iter().zip(&y) {
            for (xu, &zu) in xs.iter_mut().zip(zf) {
                *xu += zu * yf;
            }
        }
        let fill_nets = nets(&xs)?;
        let cs = conditions(&fill_nets)?;
        let mut worst = 0.0;
        for &c in &cs {
            worst = smax(worst, c.abs());
        }
        if !(worst <= 1.0e-3 * tol) {
            continue;
        }

        // no fold: every patch's normal keeps to one side, and each setback patch's agrees
        // with its round's side's interior part where they meet
        for net in &fill_nets {
            let (_, su, sv) = first(net, 0.5, 0.5)?;
            let mid = su.cross(sv);
            for a in 0..=8 {
                for b in 0..=8 {
                    let (_, su, sv) = first(net, f64::from(a) / 8.0, f64::from(b) / 8.0)?;
                    if !(su.cross(sv).dot(mid) > 0.0) {
                        return Err(Refusal::sm_err("SmBlendSetback: the fill folds"));
                    }
                }
            }
        }
        for m in 0..n {
            let (sp, su, sv) = first(el_ref(&fill_nets, m)?, 1.0, 0.25)?;
            let (qp, qu, qv) = first(el_ref(&fill_nets, n + m)?, 1.0, 0.5)?;
            if !(su.cross(sv).dot(qu.cross(qv)) > 0.0) || !((sp - qp).length() <= tol) {
                return Err(Refusal::sm_err(
                    "SmBlendSetback: the fill folds where a setback patch meets the interior",
                ));
            }
        }

        // the surfaces
        let mut patches = Vec::with_capacity(2 * n);
        for net in &fill_nets {
            let mut pts = Vec::with_capacity(net.pts.len());
            let mut ws = Vec::with_capacity(net.pts.len());
            let mut rational = false;
            for h in &net.pts {
                if !(h[3] > 0.0) {
                    return Err(sm_err().into());
                }
                pts.push(euclid(h));
                ws.push(h[3]);
                if h[3] != 1.0 {
                    rational = true;
                }
            }
            let um = [net.p + 1, net.p + 1];
            let vm = [net.q + 1, net.q + 1];
            let knots = [0.0, 1.0];
            patches.push(BSplineSurface::create_canonical(&CanonicalSurfaceData {
                u_degree: net.p,
                v_degree: net.q,
                control_points: &pts,
                form: BSplineSurfaceForm::Unspecified,
                u_multiplicities: &um,
                v_multiplicities: &vm,
                u_knots: &knots,
                v_knots: &knots,
                knot_type: KnotType::Unspecified,
                weights: if rational { Some(ws.as_slice()) } else { None },
                uv_domain: None,
            })?);
        }
        return Ok(Fill {
            patches,
            round: cycle_round,
            face: cycle_face,
            reversed: cycle_rev,
            degree: d,
        });
    }
    Err(Refusal::sm_err(
        "SmBlendSetback: no interior map up to degree 9 meets the setback patches' conditions",
    ))
}

/// `SmBlendSetback::Containment`: the most any patch of `fill` stands past a face out of the
/// corner (positive: past it): out of the material for a convex corner (`convex`), into it
/// for a concave one. A certified bound: every patch lies in the convex hull of its control
/// points (weights positive).
///
/// # Errors
/// `SM_ERR_INVALID_INPUT` for an empty fill; `SM_ERR` for a patch with a weight not positive.
pub fn containment(fill: &Fill, faces: &[Face], convex: bool) -> Result<f64> {
    if fill.patches.is_empty() {
        return Err(SolidError::invalid_input(
            OP,
            "SmBlendSetback: an empty fill",
        ));
    }
    let side = if convex { 1.0 } else { -1.0 };
    let mut worst = f64::NEG_INFINITY;
    for patch in &fill.patches {
        let canonical = patch.canonical()?;
        if canonical.weights.iter().any(|&w| !(w > 0.0)) {
            return Err(sm_err()); // a patch with a weight not positive
        }
        for &pt in &canonical.control_points {
            for face in faces {
                worst = smax(worst, side * (pt - face.point).dot(face.normal));
            }
        }
    }
    Ok(worst)
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
