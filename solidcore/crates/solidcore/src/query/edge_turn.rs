//! At a vertex where edges meet: the direction an edge leaves it in, how far one edge turns
//! from running straight on into another there, and whether an edge is convex there. The
//! measures a tangent-chain walk takes at each vertex.
//!
//! Upstream: none (not in SMLib).
//!
//! | Rust          | Upstream |
//! |---------------|----------|
//! | [`leaving`]   | (none)   |
//! | [`turn_at`]   | (none)   |
//! | [`convex_at`] | (none)   |
//!
//! Deviations: no upstream counterpart.

use crate::error::{Result, SolidError};
use crate::kernel::Kernel;
use crate::math::Vec3;
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

/// The unit direction edge `e` leaves its end `v` in: its curve's tangent there, turned to
/// point away from `v`. `None` for a closed edge, an edge without curve, or a tangent of no
/// length.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a vertex that is not an end of `e`; those of the
/// evaluation.
pub fn leaving(k: &Kernel, b: &Brep, e: EdgeId, v: VertexId) -> Result<Option<Vec3>> {
    let Some((t, at_start)) = end_at(b, e, v)? else {
        return Ok(None);
    };
    let Some(curve) = edge::curve(b, e)? else {
        return Ok(None);
    };
    // from inside the edge: from the right at its start, from the left at its end
    let d = curve.evaluate(k, t, 1, !at_start, true)?;
    let Some(&tangent) = d.get(1) else {
        return Err(invalid("a curve's evaluation without its tangent"));
    };
    let Ok(unit) = tangent.unitize() else {
        return Ok(None);
    };
    Ok(Some(if at_start { unit } else { unit * -1.0 }))
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
/// into the material). `None` for
/// an edge not between two face uses, a closed edge, an edge with no edge use on a material
/// region's side, or faces whose normal and binormal there are square to each other (tangent
/// faces: no convexity).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a vertex that is not an end of `e`; those of the
/// evaluations.
pub fn convex_at(k: &Kernel, b: &mut Brep, e: EdgeId, v: VertexId) -> Result<Option<bool>> {
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
    let options = BinormalOptions::default();
    let first = evaluate_binormal(k, b, eu0, t, false, &options)?;
    let second = evaluate_binormal(k, b, eu_r, t, false, &options)?;
    let side = second.binormal.dot(first.face_normal);
    if side == 0.0 || !side.is_finite() {
        return Ok(None);
    }
    Ok(Some(side > 0.0))
}
