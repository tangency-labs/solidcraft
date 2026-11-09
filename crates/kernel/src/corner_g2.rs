//! G2 fillet corners (`corner: G2`): the corner where three or more rounded edges meet, filled
//! with a patch that meets every face around it to the second order, instead of the rolling
//! ball's sphere (or SMLib's Coons patch), which is only tangent to the rounds and keeps its own
//! 1 / r across each seam.
//!
//! The corner is set back: SMLib runs each round whole to its cross-section a setback from the
//! vertex and leaves the corner open ([`crate::blend_smlib::fillet_setback_open`]), so the hole's
//! boundary is the rounds' sections and, on each face between two rounds, SMLib's curve from one
//! round's contact line to the next. Each hole is filled with one face
//! ([`crate::fill::matched_patch`]): a bicubic height field over the hole's plane fitted to every
//! face beside the hole — its position, its tangent plane and its normal curvature across the
//! boundary, read off the face's own surface — the rounds and the planes alike, so the patch is
//! G2 to all of them. The faces are sewn into one solid by SMLib ([`crate::stitch`]).
//!
//! Without a setback the boundary would be the three sections alone, meeting at points where
//! two rounds 90° apart ask for different curvatures; with it, the only points where two faces
//! disagree are the hole's corners, where a section ends on a plane: the round's G1 seam with the
//! plane, whose 1 / r jump every round has along its whole length, ends there, and no surface is
//! G2 to both the round and the plane at that point. The patch passes from one to the other
//! within 15% of each side from the corner (`solidcraft_geom::fill_matched::FADE_SHARE`) and
//! is G2 along the rest of every seam.

use solidcraft_geom::fill_matched::MatchedFill;

use crate::body::Body;
use crate::{KernelError, Result};

/// A corner's default setback: this times its round's radius along each edge. It must reach
/// past where the rounds meet without one (the radius, at a square corner), and SMLib's own
/// patch bulges past the faces beyond about twice the radius; one and a half leaves the patch a
/// side as long as the round's section is wide, with room either way.
pub const DEFAULT_SETBACK_FACTOR: f64 = 1.5;

/// How far the patch's normal curvature across a seam may be from the face's, clear of the hole's
/// corners: this share of the corner's round's curvature 1 / r, the jump a rolling ball makes.
pub const CURVATURE_SHARE: f64 = 5e-3;

/// The patch filling one open corner of `sheet` (a sheet from
/// [`crate::blend_smlib::fillet_setback_open`]): `hole` its edges in order, each with whether the
/// face beside it runs it forwards; `radius` the corner's round's radius, which sets the
/// curvature tolerance. The patch is a one-face sheet on the sheet's own edges.
pub(crate) fn corner_patch(sheet: &Body, hole: &[(usize, bool)], radius: f64) -> Result<(Body, MatchedFill)> {
    if !(radius.is_finite() && radius > 0.0) {
        return Err(KernelError::Invalid("a G2 corner's radius must be finite and positive".into()));
    }
    crate::fill::matched_patch(sheet, hole, CURVATURE_SHARE / radius)
}

#[cfg(test)]
#[path = "corner_g2_tests.rs"]
mod tests;
