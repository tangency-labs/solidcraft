//! How far a Join's shared section stands off the second blend, from the corner's geometry
//! at its vertex: the side of the second blend's end the section lies on decides whether
//! that gap is of first or of second order in the corner's angle (#13911).
//!
//! Upstream: none (solidcore-native; the fork's Join takes no bound, #16627 added the
//! first-order one, [`super::join_within_xsect_tolerance_deg`]).
//!
//! | Rust                     | Upstream |
//! |--------------------------|----------|
//! | [`JoinBlend`]            | none     |
//! | [`join_tolerance_deg`]   | none     |
//! | [`overlap_gap`]          | none     |
//! | [`rail_curvature`]       | none     |
//!
//! A Join ends both blends on one section, the first blend's, in the plane through the
//! corner's vertex `v` square to its edge (`FilletBrep::calc_join_section`). On the face the
//! two blends share, that section's foot is `F = v + R1 b1`: `b1` the first edge's binormal
//! into the face, `R1` the first blend's reach along the face (`(oB - oA cos phi) / sin phi`
//! for offsets `oA` off the shared face and `oB` off its other face, the faces' normals
//! `phi` apart; a round's `r tan(phi / 2)`, a chamfer's setback). Let `t2` be the second
//! edge's direction leaving `v`, and `beta` the angle the two edges make in the face
//! (`cos beta = -t1 . t2` for `t1` the first edge's direction leaving `v`).
//!
//! - **Behind** (`b1 . t2 < 0`): `F` lies `R1 sin beta` before the second blend's own end,
//!   where its surface does not reach, so the vertex stands that far off it: first order,
//!   #16627's bound (`2 size tan(a / 2)`, MEASURED there as `o a` on the concave corners
//!   of its plates, where the section lies behind).
//! - **Over** (`b1 . t2 >= 0`): `F` lies over the second blend, whose own section in the
//!   plane (its ball there, found on its own faces) has its foot on the same line through
//!   `v`, at `R2 / cos beta` for straight edges. The vertex stands at most
//!   `|R1 - R2 / cos beta|` off that foot, plus the second blend's rail bending away from its
//!   tangent over the `R sin beta` it runs: [`overlap_gap`]. With equal reaches that is
//!   `R (sec beta - 1)`, second order: at the 3.4e-3 degrees of #13911, `1.8e-9 R`.
//!
//! So where the section lies over the second blend the Join is taken while [`overlap_gap`]
//! stays below the intersection tolerance, at any angle the solvers take as a continuation,
//! and behind it only within #16627's bound. The order of the two blends does not matter:
//! to first order `b1 . t2 = b2 . t1`, and the decision takes both.
//!
//! The bend term is ARGUED, not certified: it takes each rail's curvature at the vertex
//! ([`rail_curvature`], from the edge's curvature in the face there) as its bound over the
//! `R sin beta` it runs, which a rail whose curvature changes faster than that over a few
//! thousandths of a millimetre exceeds. On a curved shared face the edge's curvature in the
//! face is taken from its 3D acceleration along the binormal, and the face's own bending
//! over that run is not bounded here. Where any quantity is not finite, a reach is negative,
//! the faces' normals are parallel, a rail folds (its edge bends toward the face with a radius
//! no larger than the reach) or the run is not short against the rail's radius, the decision
//! falls back to #16627's bound, the conservative side.
//!
//! Deviations: none (no upstream counterpart).

use core::cmp::Ordering;

use crate::error::Result;
use crate::kernel::Kernel;
use crate::math::{Vec3, EFF_ZERO};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use crate::topology::{edge, Brep, EdgeuseId, FaceId, VertexId};

/// One blend of a corner as [`join_tolerance_deg`] reads it: its edge use and, for each of
/// its two faces, the face and the blend's offset off it (the solver's offset surfaces).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JoinBlend {
    /// The blended edge use.
    pub eu: EdgeuseId,
    /// The blend's two faces with its offset off each (an offset's absolute distance).
    pub faces: [(Option<FaceId>, f64); 2],
}

/// The blend's geometry at the corner's vertex on the face both blends share.
#[derive(Clone, Copy, Debug)]
struct AtVertex {
    /// The edge's unit direction leaving the vertex.
    out: Vec3,
    /// The edge's unit binormal into the shared face there.
    binormal: Vec3,
    /// The blend's reach along the shared face (its section's foot's distance from the edge).
    reach: f64,
    /// The edge's curvature in the face there, positive where it bends toward the face.
    bend: f64,
}

/// The curvature of a rail offset `reach` into the face from an edge whose curvature in the
/// face is `bend` (positive bending toward the face): `bend / (1 - reach bend)` in size.
/// `None` where the rail bends toward its centre with `reach bend >= 1` (the offset folds
/// there: no rail), or for a non-finite input or a negative reach.
#[must_use]
pub fn rail_curvature(bend: f64, reach: f64) -> Option<f64> {
    if !(bend.is_finite() && reach.is_finite() && reach >= 0.0) {
        return None;
    }
    if bend > 0.0 {
        (reach * bend < 1.0).then(|| bend / (1.0 - reach * bend))
    } else {
        Some(bend.abs() / (1.0 + reach * bend.abs()))
    }
}

/// The vertex stands at most this far off the second blend's section where the shared
/// section lies over it (module docs): `|R1 - R2 / cos beta|`, either way round, plus each
/// rail's bend `kr s^2 / 2` over `s = R sin beta` (`kr` the larger [`rail_curvature`],
/// `R` the larger reach). `None` where `cos beta` is not positive, a reach or bend is not
/// one [`rail_curvature`] takes, or the run is not short against the rail's radius
/// (`kr s >= 1/2`), where the quadratic bend no longer bounds it.
#[must_use]
pub fn overlap_gap(
    reaches: [f64; 2],
    cos_beta: f64,
    sin_beta: f64,
    bends: [f64; 2],
) -> Option<f64> {
    let [r1, r2] = reaches;
    if !(cos_beta > 0.0 && sin_beta.is_finite()) {
        return None;
    }
    let reach = r1.max(r2);
    let k1 = rail_curvature(bends[0], r1)?;
    let k2 = rail_curvature(bends[1], r2)?;
    let run = reach * sin_beta.abs();
    let kr = k1.max(k2);
    if (kr * run).partial_cmp(&0.5) != Some(Ordering::Less) {
        return None;
    }
    let apart = (r1 - r2 / cos_beta).abs().max((r2 - r1 / cos_beta).abs());
    let gap = apart + 0.5 * kr * run * run;
    gap.is_finite().then_some(gap)
}

/// The blend's [`AtVertex`] on `shared`, or `None` where it cannot be read (no edge use of
/// the blend's edge on that face, a degenerate tangent or binormal, faces whose normals are
/// parallel there, a negative reach).
fn at_vertex(
    k: &Kernel,
    b: &mut Brep,
    vertex: VertexId,
    blend: &JoinBlend,
    shared: FaceId,
) -> Result<Option<AtVertex>> {
    let Some(e) = eu1::edge(b, blend.eu)? else {
        return Ok(None);
    };
    let ivl = edge::interval(b, e)?;
    let at_start = edge::start_vertex(b, e)? == Some(vertex);
    let param = if at_start { ivl.min() } else { ivl.max() };
    let Some(curve) = edge::curve(b, e)?.cloned() else {
        return Ok(None);
    };
    let derivs = curve.evaluate(k, param, 2, !at_start, true)?;
    let (Some(&d1), Some(&d2)) = (derivs.get(1), derivs.get(2)) else {
        return Ok(None);
    };
    let speed_sq = d1.length_squared();
    if speed_sq.partial_cmp(&(EFF_ZERO * EFF_ZERO)) != Some(Ordering::Greater) {
        return Ok(None);
    }
    let Ok(tangent) = d1.unitize() else {
        return Ok(None);
    };
    let out = if at_start { tangent } else { -tangent };

    // the edge use on the shared face and the one on the blend's other face
    let Some(radial) = eu1::radial(b, blend.eu)? else {
        return Ok(None);
    };
    let (on_shared, on_other) = if eu1::face(b, blend.eu)? == Some(shared) {
        (blend.eu, radial)
    } else if eu1::face(b, radial)? == Some(shared) {
        (radial, blend.eu)
    } else {
        return Ok(None);
    };
    let opts = BinormalOptions::default();
    let s = evaluate_binormal(k, b, on_shared, param, false, &opts)?;
    let o = evaluate_binormal(k, b, on_other, param, false, &opts)?;
    let (Ok(binormal), Ok(n_shared), Ok(n_other)) = (
        s.binormal.unitize(),
        s.face_normal.unitize(),
        o.face_normal.unitize(),
    ) else {
        return Ok(None);
    };
    let cos_phi = n_shared.dot(n_other);
    let sin_phi = n_shared.cross(n_other).length();
    if sin_phi.partial_cmp(&EFF_ZERO) != Some(Ordering::Greater) {
        return Ok(None);
    }
    let other_face = eu1::face(b, on_other)?;
    let offset_on = |face: Option<FaceId>| {
        blend
            .faces
            .iter()
            .find(|(f, _)| f.is_some() && *f == face)
            .map(|&(_, o)| o)
    };
    let (Some(o_shared), Some(o_other)) = (offset_on(Some(shared)), offset_on(other_face)) else {
        return Ok(None);
    };
    let reach = (o_other - o_shared * cos_phi) / sin_phi;
    if !(reach.is_finite() && reach >= 0.0) {
        return Ok(None);
    }
    // the acceleration along the binormal over the speed squared: the edge's curvature in
    // the face (the binormal is square to the tangent)
    let bend = d2.dot(binormal) / speed_sq;
    Ok(Some(AtVertex {
        out,
        binormal,
        reach,
        bend,
    }))
}

/// The angle (degrees) within which the faces across the corner's side edges may meet for
/// its two blends to Join: the solvers' tangency tolerance `tangency_tol_deg` where the
/// shared section lies over the second blend and [`overlap_gap`] there stays strictly below
/// the intersection tolerance `xsect_tol` (less #16627's rounding margin, for the larger
/// reach, and as much again for the gap's own rounding), else `behind_deg`, #16627's bound
/// ([`super::join_within_xsect_tolerance_deg`]). `behind_deg` too where the two blends share
/// no single face, or any quantity cannot be read (module docs).
///
/// # Errors
///
/// Those of the topology queries, the curve evaluation and `evaluate_binormal`.
pub fn join_tolerance_deg(
    k: &Kernel,
    b: &mut Brep,
    vertex: VertexId,
    blends: [&JoinBlend; 2],
    xsect_tol: f64,
    tangency_tol_deg: f64,
    behind_deg: f64,
) -> Result<f64> {
    if !(xsect_tol.is_finite() && xsect_tol > 0.0) {
        return Ok(behind_deg);
    }
    let [first, second] = blends;
    // the one face both blends roll on
    let shared: Vec<FaceId> = first
        .faces
        .iter()
        .filter_map(|&(f, _)| f)
        .filter(|f| second.faces.iter().any(|&(g, _)| g == Some(*f)))
        .collect();
    let [shared] = shared.as_slice() else {
        return Ok(behind_deg);
    };
    let (Some(one), Some(two)) = (
        at_vertex(k, b, vertex, first, *shared)?,
        at_vertex(k, b, vertex, second, *shared)?,
    ) else {
        return Ok(behind_deg);
    };
    // over the other blend, either way round (equal to first order); a NaN is not over
    let over = one.binormal.dot(two.out) >= 0.0 && two.binormal.dot(one.out) >= 0.0;
    if !over {
        return Ok(behind_deg);
    }
    let cos_beta = -one.out.dot(two.out);
    let sin_beta = one.out.cross(two.out).length();
    let Some(gap) = overlap_gap(
        [one.reach, two.reach],
        cos_beta,
        sin_beta,
        [one.bend, two.bend],
    ) else {
        return Ok(behind_deg);
    };
    // #16627's margin for the budget, and as much again for the gap's own rounding
    let eps = f64::EPSILON;
    let reach = one.reach.max(two.reach);
    let budget = xsect_tol * (1.0 - 16.0 * eps) - 8.0 * eps * reach;
    if gap * (1.0 + 16.0 * eps) + 8.0 * eps * reach < budget {
        Ok(tangency_tol_deg)
    } else {
        Ok(behind_deg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_overlap_gap_is_of_second_order() {
        // equal reaches, straight edges: R (sec beta - 1)
        let beta = 5.9e-5_f64;
        let gap = overlap_gap([1.0, 1.0], beta.cos(), beta.sin(), [0.0, 0.0]).unwrap();
        assert!((gap - (1.0 / beta.cos() - 1.0)).abs() <= 1e-24, "{gap}");
        assert!(gap < 2e-9);
        // a rail bent toward the face adds kr (R sin beta)^2 / 2, kr = k / (1 - R k)
        let gap = overlap_gap([1.0, 1.0], beta.cos(), beta.sin(), [0.4, 0.0]).unwrap();
        let kr = 0.4 / (1.0 - 0.4);
        let want = 1.0 / beta.cos() - 1.0 + 0.5 * kr * beta.sin().powi(2);
        assert!((gap - want).abs() <= 1e-24, "{gap} {want}");
        // bent away from the face: k / (1 + R k)
        let gap = overlap_gap([1.0, 1.0], beta.cos(), beta.sin(), [0.0, -0.625]).unwrap();
        let kr = 0.625 / (1.0 + 0.625);
        let want = 1.0 / beta.cos() - 1.0 + 0.5 * kr * beta.sin().powi(2);
        assert!((gap - want).abs() <= 1e-24, "{gap} {want}");
        // unequal reaches stand apart by their difference: first order
        let gap = overlap_gap([1.0, 0.9], 1.0, 0.0, [0.0, 0.0]).unwrap();
        assert!((gap - 0.1).abs() < 1e-15);
    }

    #[test]
    fn the_overlap_gap_refuses_what_it_cannot_bound() {
        assert_eq!(overlap_gap([1.0, 1.0], 0.0, 1.0, [0.0, 0.0]), None);
        assert_eq!(overlap_gap([1.0, 1.0], -0.5, 0.8, [0.0, 0.0]), None);
        assert_eq!(overlap_gap([f64::NAN, 1.0], 1.0, 0.0, [0.0, 0.0]), None);
        assert_eq!(overlap_gap([1.0, -1.0], 1.0, 0.0, [0.0, 0.0]), None);
        assert_eq!(overlap_gap([1.0, 1.0], f64::NAN, 0.0, [0.0, 0.0]), None);
        assert_eq!(
            overlap_gap([1.0, 1.0], 1.0, 0.0, [f64::INFINITY, 0.0]),
            None
        );
        // a rail folded at its reach, or bent too much over its run
        assert_eq!(overlap_gap([1.0, 1.0], 1.0, 0.0, [1.0, 0.0]), None);
        assert!(overlap_gap([1.0, 1.0], 1.0, 0.0, [0.999, 0.0]).is_some());
        assert_eq!(overlap_gap([1.0, 1.0], 0.6, 0.8, [0.9, 0.0]), None);
        // bent away, any curvature is bounded
        assert!(overlap_gap([1.0, 1.0], 1.0, 0.0, [-1e6, 0.0]).is_some());
    }

    #[test]
    fn a_rail_s_curvature_follows_its_side() {
        assert_eq!(rail_curvature(0.0, 1.0), Some(0.0));
        assert_eq!(rail_curvature(0.25, 1.0), Some(0.25 / 0.75));
        assert_eq!(rail_curvature(-0.25, 1.0), Some(0.25 / 1.25));
        assert_eq!(rail_curvature(1.0, 1.0), None);
        assert_eq!(rail_curvature(0.5, 1.0), Some(1.0));
        assert_eq!(rail_curvature(f64::NAN, 1.0), None);
        assert_eq!(rail_curvature(0.1, -1.0), None);
    }
}
