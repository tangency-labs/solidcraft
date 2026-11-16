//! The offset of a blend by its definition (#2487): built from the definition S, never by
//! offsetting its net B.
//!
//! A rolling-ball round of radius r on supports F1 and F2 is the envelope of a ball whose
//! centre runs on the spine. Its offset by d along its normal is the envelope of the ball
//! of radius `r' = r + tau d` about the same centres (`tau` is +1 where the round's normal
//! points away from the ball's centre, -1 where it points at it), and that ball touches
//! each support offset by `sigma_i d` along the support's own normal, `sigma_i = -tau
//! side_i`: at the foot the round's normal is `-tau side_i` times the support's. So the
//! offset is the round of radius `r'` on the supports so offset, on the same spine:
//!
//! - a [`ProceduralBlend`] of the ball's circle (order 1, one radius) offsets to the
//!   procedural blend of radius `r'` on the offset supports, the same leader, seeds and
//!   map; its net is built on the definition's stations and certified
//!   ([`crate::blend::fillet_section::recertify::net_of_definition`]), its bound recorded,
//!   and the result re-certified against it;
//! - a round of an exact class ([`ExactRound`]) offsets to its kind's own exact offset (a
//!   cylinder of r to the cylinder of `r'`), which carries the definition of radius `r'`
//!   on the offset supports, re-certified (an identity: the supports classified again give
//!   the class, the offset on its surface).
//!
//! Where `r'` is not positive (an inward offset at or past the radius) the round collapses
//! onto its spine: no surface, as a cylinder's offset collapses
//! ([`crate::surface::surface::Surface::offset_collapses`]). A section of order 2 and up,
//! or a radius law, has no such identity and is not built here; nor is a round on a
//! B-spline face, whose offset is an offset surface the round's certificate does not
//! enclose. Those offset as the offset surface of S itself (the blend, evaluated from its
//! definition).
//!
//! `tau` is read off the round's curvature: across the section the round is the ball's
//! circle, a principal curvature of `-tau / r` along the round's normal.
//!
//! Not upstream. Deviations: none (no upstream counterpart). No recursion: a support that
//! is itself a blend is offset by its own kind's offset, which comes back here once, at
//! most [`super::BLEND_MAX_DEPTH`] deep.

use super::exact::ExactRound;
use super::{BlendDefinitionInput, ProceduralBlend};
use crate::blend::fillet_section::recertify::net_of_definition;
use crate::blend::fillet_section::section::GnSectionFsg;
use crate::blend::fillet_section::station::as_b_spline;
use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::kernel::Kernel;
use crate::math::{Point2, Vec3};
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::surface::surface::{Surface, SurfaceKind};

const OP: &str = "surface::procedural_blend::offset";

/// The offset of a round by its definition.
#[derive(Debug)]
pub enum BlendOffset {
    /// The round of the moved radius on the offset supports.
    Round(Box<Surface>),
    /// The round collapses onto its spine (its radius moved to zero or past it).
    Collapsed,
}

/// The ball's side of the round `s` at `uv` along its normal `S_u x S_v`: +1 where the
/// normal points away from the ball's centre, -1 where it points at it. Across the
/// section the round is the ball's circle of radius `r`, a principal curvature of
/// `-tau / r`; the principal curvature nearest `+-1 / r` names `tau`.
///
/// # Errors
///
/// `InvalidInput` where no principal curvature is the ball's (the surface is not a round
/// of radius `r` there); those of the evaluation.
fn ball_side(k: &Kernel, s: &Surface, uv: Point2, r: f64) -> Result<f64> {
    let d = s.evaluate(k, uv, 2, 2, EvaluateFlags::default())?;
    let at = |i: usize, j: usize| -> Vec3 {
        d.get(i)
            .and_then(|row| row.get(j))
            .copied()
            .unwrap_or(Vec3::new(f64::NAN, f64::NAN, f64::NAN))
    };
    let (su, sv) = (at(1, 0), at(0, 1));
    let n = su.cross(sv);
    let len = n.length();
    let not_a_round = || SolidError::invalid_input(OP, "the surface is not its round's ball's");
    if !(len > 0.0 && len.is_finite()) {
        return Err(not_a_round());
    }
    let n = n * (1.0 / len);
    let (e, f, g) = (su.dot(su), su.dot(sv), sv.dot(sv));
    let (l, m, nn) = (at(2, 0).dot(n), at(1, 1).dot(n), at(0, 2).dot(n));
    let det = e * g - f * f;
    let mean = (e * nn - 2.0 * f * m + g * l) / (2.0 * det);
    let gauss = (l * nn - m * m) / det;
    let disc = (mean * mean - gauss).max(0.0).sqrt();
    let curvatures = [mean + disc, mean - disc];
    // tau = -sign of the ball's curvature: the one of +-1/r nearest a principal curvature
    let away = curvatures
        .iter()
        .fold(f64::INFINITY, |a, &c| a.min((c + 1.0 / r).abs()));
    let toward = curvatures
        .iter()
        .fold(f64::INFINITY, |a, &c| a.min((c - 1.0 / r).abs()));
    // (the ball's curvature is one of them to rounding; the other sign is 2 / r off)
    let near = 0.5 / r;
    if away < near && away < toward {
        Ok(1.0)
    } else if toward < near && toward < away {
        Ok(-1.0)
    } else {
        Err(not_a_round())
    }
}

/// The middle of a surface's natural domain.
fn middle(k: &Kernel, s: &Surface) -> Result<Point2> {
    Ok(s.natural_uv_domain_with_nurb(k)?.evaluate(0.5, 0.5))
}

/// The support offset by `distance` along its own normal.
///
/// # Errors
///
/// `InvalidInput` where the support collapses (a cylinder offset onto its axis); those of
/// its offset.
fn offset_support(k: &Kernel, s: &Surface, distance: f64, tol: f64) -> Result<Surface> {
    s.create_offset_surface(k, distance, tol)?.ok_or_else(|| {
        SolidError::invalid_input(OP, "a support of the round collapses at this offset")
    })
}

/// The new radius `r + tau d`, or `None` where the round collapses (at most `tol`).
fn moved_radius(r: f64, tau: f64, distance: f64, tol: f64) -> Option<f64> {
    let moved = r + tau * distance;
    (moved > tol).then_some(moved)
}

/// Whether the offset of a procedural round `p` (the definition of `surface`) by
/// `distance` collapses onto its spine ([`procedural_offset`]'s [`BlendOffset::Collapsed`]),
/// or `None` where its definition has no offset identity.
///
/// # Errors
///
/// `InvalidInput` for a surface that is not its round's ball's; those of the evaluation.
pub fn procedural_collapses(
    k: &Kernel,
    surface: &Surface,
    p: &ProceduralBlend,
    distance: f64,
    tol: f64,
) -> Result<Option<bool>> {
    if !(p.order() == 1 && p.law().is_none()) {
        return Ok(None);
    }
    let r = p.definition().offsets[0];
    let tau = ball_side(k, surface, middle(k, surface)?, r)?;
    Ok(Some(moved_radius(r, tau, distance, tol).is_none()))
}

/// The offset of a procedural round's surface by `distance`, or `None` where its
/// definition has no offset identity (a section of order 2 and up, or a radius law).
///
/// # Errors
///
/// `InvalidInput` for a surface that is not its round's ball's, a support that collapses
/// and a net not certified; those of the supports' offsets, the net's construction and the
/// re-certification.
pub fn procedural_offset(
    k: &Kernel,
    surface: &Surface,
    p: &ProceduralBlend,
    distance: f64,
    tol: f64,
) -> Result<Option<BlendOffset>> {
    if !(p.order() == 1 && p.law().is_none()) {
        return Ok(None);
    }
    let def = p.definition();
    let r = def.offsets[0];
    let tau = ball_side(k, surface, middle(k, surface)?, r)?;
    let Some(moved) = moved_radius(r, tau, distance, tol) else {
        return Ok(Some(BlendOffset::Collapsed));
    };
    let [s0, s1] = p.supports();
    let supports = [
        offset_support(k, s0, -tau * def.sides[0] * distance, tol)?,
        offset_support(k, s1, -tau * def.sides[1] * distance, tol)?,
    ];
    // the certificate encloses NURBS supports only: a B-spline support's offset is an
    // offset surface, on which the round of r' is not certified (its offset is then the
    // offset surface of S itself, evaluated from the definition)
    for support in &supports {
        if as_b_spline(k, support)?.is_none() {
            return Ok(None);
        }
    }
    let [seed0, seed1] = &def.seeds;
    let input = BlendDefinitionInput {
        supports: [&supports[0], &supports[1]],
        sides: def.sides,
        offsets: [moved, moved],
        law: None,
        order: 1,
        thumbweight: 0.0,
        leader: &def.leader,
        seeds: [seed0, seed1],
        bound: def.bound,
        // an order-1 round: a chamfer's sizes belong to order 0 only (#2486)
        chamfer: None,
    };
    let valid = |n: u32, w: f64| GnSectionFsg::is_valid(n as usize, w);
    // the definition on the old net (its net is built from the definition alone), then on
    // its own net, over its own (s, t)
    let draft = ProceduralBlend::create(k, &p.net, &input, valid)?;
    let (net, bound) = net_of_definition(k, &draft, def.bound)?;
    let mut made = Surface::from(ProceduralBlend::create(
        k,
        &net,
        &BlendDefinitionInput { bound, ..input },
        valid,
    )?);
    into_parameters_of(k, &mut made, p)?;
    match made.procedural_blend() {
        Some(b) => {
            b.recertify(k)?;
        }
        None => {
            return Err(SolidError::surface_failed(
                OP,
                "the offset round is not a procedural blend",
            ))
        }
    }
    Ok(Some(BlendOffset::Round(Box::new(made))))
}

/// Puts `made`, a procedural blend over its definition's own (s, t), into the parameters
/// of `like`: `like`'s map from its (u, v) to (s, t), the net swapped, reversed and
/// reparametrized to match, so that the face's uv curves on `like` lie on `made` at the
/// same (u, v). The map is then `like`'s exactly (the net's knots are the reparametrized
/// ones, to rounding, which the re-certification takes in).
fn into_parameters_of(k: &Kernel, made: &mut Surface, like: &ProceduralBlend) -> Result<()> {
    let (a, b, swapped) = like.definition_map();
    if swapped {
        made.swap_uv(k)?;
    }
    let SurfaceKind::ProceduralBlend(pb) = made.kind_mut() else {
        return Err(SolidError::surface_failed(
            OP,
            "the made surface is not a procedural blend",
        ));
    };
    // (p, q) drive (s, t): u and v, or v and u when swapped
    for (c, (&ac, &bc)) in a.iter().zip(&b).enumerate() {
        let drives_u = (c == 0) != swapped;
        let param = if drives_u {
            SurfParamType::U
        } else {
            SurfParamType::V
        };
        let (_, now_b, _) = pb.definition_map();
        let now_bc = now_b.get(c).copied().unwrap_or(f64::NAN);
        if (now_bc > 0.0) != (bc > 0.0) {
            pb.reverse(param)?;
        }
        let (now_a, now_b, _) = pb.definition_map();
        let (Some(&na), Some(&nb)) = (now_a.get(c), now_b.get(c)) else {
            return Err(SolidError::surface_failed(
                OP,
                "the definition map has no entry for the parameter",
            ));
        };
        // the net's range in this parameter, then the same s in like's parameter
        let dom = pb.net().natural_uv_domain();
        let (p0, p1) = if drives_u {
            (dom.min().x, dom.max().x)
        } else {
            (dom.min().y, dom.max().y)
        };
        let to_like = |p: f64| (na + nb * p - ac) / bc;
        let (l0, l1) = (to_like(p0), to_like(p1));
        let (lo, hi) = if l0 <= l1 { (l0, l1) } else { (l1, l0) };
        let new_domain = if drives_u {
            Extent2d::from_points(Point2::new(lo, dom.min().y), Point2::new(hi, dom.max().y))
        } else {
            Extent2d::from_points(Point2::new(dom.min().x, lo), Point2::new(dom.max().x, hi))
        };
        pb.reparameterize(&new_domain)?;
    }
    pb.def.map_a = a;
    pb.def.map_b = b;
    pb.def.swapped = swapped;
    Ok(())
}

/// The offset of an exact round's surface `surface` (`offset`, its kind's own exact
/// offset by `distance`, or `None` where that collapses) carrying the definition of the
/// moved radius on the offset supports, re-certified.
///
/// # Errors
///
/// `InvalidInput` for a surface that is not its round's ball's, a support that collapses
/// and an offset not on the class's surface of the offset supports; those of the
/// supports' offsets and the re-certification.
pub fn exact_offset(
    k: &Kernel,
    surface: &Surface,
    round: &ExactRound,
    offset: Option<Surface>,
    distance: f64,
    tol: f64,
) -> Result<BlendOffset> {
    let r = round.radius();
    let tau = ball_side(k, surface, middle(k, surface)?, r)?;
    let (Some(moved), Some(mut offset)) = (moved_radius(r, tau, distance, tol), offset) else {
        return Ok(BlendOffset::Collapsed);
    };
    let mut inputs = *round.inputs();
    let mut supports = Vec::new();
    for (i, o) in inputs.offsets.iter_mut().enumerate() {
        let support = round
            .support(i)
            .ok_or_else(|| SolidError::surface_failed(OP, "the round has no support to offset"))?;
        // the centre's signed offset from the support is side_i r; the support moves by
        // -tau side_i d, so the centre's offset from it is side_i r'
        let side = if *o >= 0.0 { 1.0 } else { -1.0 };
        supports.push(offset_support(k, support, -tau * side * distance, tol)?);
        *o = side * moved;
    }
    let (Some(s0), Some(s1)) = (supports.first(), supports.get(1)) else {
        return Err(SolidError::surface_failed(
            OP,
            "the round has fewer than two supports",
        ));
    };
    let moved_round =
        ExactRound::create(k, round.class(), [s0, s1], moved, round.leader(), inputs)?;
    moved_round.recertify(k, &offset)?;
    offset.set_exact_round(Some(moved_round));
    Ok(BlendOffset::Round(Box::new(offset)))
}
