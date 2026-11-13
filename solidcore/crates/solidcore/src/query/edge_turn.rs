//! At a vertex where edges meet: the direction an edge leaves it in, how far one edge turns
//! from running straight on into another there, and whether an edge is convex there. The
//! measures a tangent-chain walk takes at each vertex.
//!
//! Upstream: none (not in SMLib).
//!
//! | Rust          | Upstream |
//! |---------------|----------|
//! | [`leaving`], [`leaving_derivative`] | (none) |
//! | [`turn_at`]   | (none)   |
//! | [`convex_at`] | (none)   |
//!
//! Deviations: no upstream counterpart.

use crate::blend::fillet_section::interval::Ivl;
use crate::error::{Result, SolidError};
use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::nurbs::math::UNIT;
use crate::query::tangent_edge::{cross_ivl, dot_ivl, face_use_normal, norm_ivl};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use crate::topology::{edge, Brep, EdgeId, VertexId};

const OP: &str = "query::edge_turn";

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// Edge `e`'s parameter at its end `v`, and whether `v` is its start; `None` for a closed
/// edge (both ends `v`).
fn end_at(b: &Brep, e: EdgeId, v: VertexId) -> Result<Option<(f64, bool)>> {
    let start = edge::start_vertex(b, e)?;
    let end = edge::end_vertex(b, e)?;
    if start == end {
        return Ok(None);
    }
    let ivl = edge::interval(b, e)?;
    if start == Some(v) {
        Ok(Some((ivl.min(), true)))
    } else if end == Some(v) {
        Ok(Some((ivl.max(), false)))
    } else {
        Err(invalid("a vertex that is not an end of the edge"))
    }
}

/// The derivative of edge `e`'s curve at its start (`at_start`) or its end, taken from inside
/// the edge and turned to point away from that end (not unit): the one evaluation every
/// "direction an edge leaves its vertex in" uses ([`leaving`], the mixed corner's). `None` for
/// an edge without curve.
///
/// Inside the edge is one-sided: `from_left` selects the half-open span `[U_i, U_(i+1))`
/// ([`crate::nurbs::knots::Side::Left`]), the one above a knot, so it is taken at the start
/// and not at the end, where the span below is. Across a knot where the curve kinks, the
/// other side would read the neighbouring segment's tangent.
///
/// # Errors
///
/// Those of the evaluation; [`SolidError::InvalidInput`] for an evaluation without its first
/// derivative.
pub fn leaving_derivative(k: &Kernel, b: &Brep, e: EdgeId, at_start: bool) -> Result<Option<Vec3>> {
    match leaving_tangent(k, b, e, at_start)? {
        Leaving::NoCurve => Ok(None),
        Leaving::NoTangent => Err(invalid("a curve's evaluation without its tangent")),
        Leaving::Tangent(d) => Ok(Some(d)),
    }
}

/// What [`leaving_derivative`]'s evaluation found, for a caller that names each case in its
/// own refusals (the mixed corner's typed errors).
pub(crate) enum Leaving {
    /// The edge has no curve.
    NoCurve,
    /// The curve's evaluation came back without its first derivative.
    NoTangent,
    /// The derivative inside the edge, turned away from the vertex.
    Tangent(Vec3),
}

/// [`leaving_derivative`], each case named ([`Leaving`]).
///
/// # Errors
///
/// Those of the evaluation.
pub(crate) fn leaving_tangent(k: &Kernel, b: &Brep, e: EdgeId, at_start: bool) -> Result<Leaving> {
    let Some(curve) = edge::curve(b, e)? else {
        return Ok(Leaving::NoCurve);
    };
    let ivl = edge::interval(b, e)?;
    let t = if at_start { ivl.min() } else { ivl.max() };
    let d = curve.evaluate(k, t, 1, at_start, true)?;
    let Some(&tangent) = d.get(1) else {
        return Ok(Leaving::NoTangent);
    };
    Ok(Leaving::Tangent(if at_start { tangent } else { -tangent }))
}

/// The unit direction edge `e` leaves its end `v` in: its curve's tangent there from inside
/// the edge ([`leaving_derivative`]), turned to point away from `v`. `None` for a closed edge
/// (which end is meant is not said; the mixed corner takes the start), an edge without curve,
/// or a tangent of no length.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a vertex that is not an end of `e`; those of the
/// evaluation.
pub fn leaving(k: &Kernel, b: &Brep, e: EdgeId, v: VertexId) -> Result<Option<Vec3>> {
    let Some((_, at_start)) = end_at(b, e, v)? else {
        return Ok(None);
    };
    let Some(d) = leaving_derivative(k, b, e, at_start)? else {
        return Ok(None);
    };
    Ok(d.unitize().ok())
}

/// How far (radians, 0 to pi) edge `f` turns at their shared end `v` from running straight on
/// from edge `e`: the angle between the direction `e` arrives at `v` in and the one `f` leaves
/// it in. 0 where `f` continues `e` tangentially. `None` where either direction is undefined
/// ([`leaving`]).
///
/// # Errors
///
/// Those of [`leaving`].
pub fn turn_at(k: &Kernel, b: &Brep, e: EdgeId, f: EdgeId, v: VertexId) -> Result<Option<f64>> {
    let (Some(arrives), Some(leaves)) = (leaving(k, b, e, v)?, leaving(k, b, f, v)?) else {
        return Ok(None);
    };
    let arrives = arrives * -1.0;
    Ok(Some(
        arrives.cross(leaves).length().atan2(arrives.dot(leaves)),
    ))
}

/// Whether edge `e` is convex at its end `v` (its faces meet at less than 180 degrees inside
/// the material there): the direction from the edge into its second face, square to it,
/// points along its first face's inward normal (the face use normals of the edge use on the
/// material's side, its face use's region not void, and of its radial partner, which point
/// into the material). `None` for an edge not between two face uses, a closed edge, an edge
/// with no edge use on a material region's side, faces not proved to part by more than
/// `smooth` radians (in `(0, pi / 2)`) there, or a side not proved apart from 0: tangent
/// faces have no convexity. The faces of a stored body are tangent only to the rounding of
/// its pcurves, so the sign of the side alone, past the arithmetic's rounding, can still be
/// that noise; the caller's smooth angle (the one its chain joins edges by) says when the
/// faces count as tangent. So `None` means: tangent within the angle asked, or not
/// decidable. The normals are enclosed (`query::tangent_edge`'s `F`); the
/// edge's tangent is taken as its evaluator gives it, and the evaluator's binormal only fixes
/// the sense of the direction into the second face.
///
/// Two ported convexity tests stay beside it, each with upstream's numerics:
/// [`crate::topology::edgeuse::p2::is_convex_radial_sector`] (`IsConvexRadialSector`) decides
/// a whole edge from samples along it with a degree tolerance, and the blend corner's
/// `continuing_edges` compares convexity by its port's rule. This one is a certified answer
/// at one vertex, and says `None` where the faces are tangent rather than guess a side.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a vertex that is not an end of `e` or a smooth angle out
/// of range; those of the evaluations.
pub fn convex_at(
    k: &Kernel,
    b: &mut Brep,
    e: EdgeId,
    v: VertexId,
    smooth: f64,
) -> Result<Option<bool>> {
    if !(smooth > 0.0 && smooth < std::f64::consts::FRAC_PI_2) {
        return Err(invalid("a smooth angle not in (0, pi / 2)"));
    }
    if !edge::is_manifold(b, e)? || eu1::edge_is_lamina(b, e)? {
        return Ok(None);
    }
    let Some((t, _)) = end_at(b, e, v)? else {
        return Ok(None);
    };
    // the edge use on the material's side: its face use and its radial partner's bound the
    // material, and their normals point into it (the primary edge use may be on the void's,
    // whose normals point out)
    let mut eu0 = None;
    for eu in edge::edgeuses(b, e, None, None)? {
        let Some(fu) = eu1::faceuse(b, eu)? else {
            continue;
        };
        if let Some(r) = eu1::faceuse_region(b, fu)? {
            if b.region(r)?.is_void() == 0 {
                eu0 = Some(eu);
                break;
            }
        }
    }
    let Some(eu0) = eu0 else {
        return Ok(None);
    };
    let Some(eu_r) = eu1::radial(b, eu0)? else {
        return Ok(None);
    };
    // the two face use normals, enclosed (positive multiples of the unit normals, into the
    // material on this side)
    let (Some(na), Some(nc)) = (
        face_use_normal(k, b, eu0, t)?,
        face_use_normal(k, b, eu_r, t)?,
    ) else {
        return Ok(None);
    };
    // faces within `smooth` of each other: tangent, no convexity. They part by more only where
    // a dot product of at most 0 or |na x nc| > tan(smooth) (na . nc) is proved (the tangent
    // within 2 ulps, and the product one rounding: 8 u more)
    let (c, d) = (norm_ivl(cross_ivl(na, nc)), dot_ivl(na, nc));
    let parted = d.hi <= 0.0 || c.lo > smooth.tan() * (1.0 + 8.0 * UNIT) * d.hi;
    if !parted {
        return Ok(None);
    }
    // the direction from the edge into the second face, square to it: its normal across the
    // edge's tangent, turned to the evaluator's binormal (which only fixes its sense)
    let options = BinormalOptions::default();
    let second = evaluate_binormal(k, b, eu_r, t, false, &options)?;
    let tangent = [second.tangent.x, second.tangent.y, second.tangent.z].map(Ivl::from);
    let mut into = cross_ivl(nc, tangent);
    let along = into[0].mid() * second.binormal.x
        + into[1].mid() * second.binormal.y
        + into[2].mid() * second.binormal.z;
    if along < 0.0 {
        into = into.map(|x| Ivl::from(0.0) - x);
    }
    let side = dot_ivl(into, na);
    // tangent faces there: the enclosure holds 0, no convexity
    if side.lo <= 0.0 && side.hi >= 0.0 {
        return Ok(None);
    }
    Ok(Some(side.lo > 0.0))
}
