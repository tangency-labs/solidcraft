//! Stake welds: a laser or electron beam weld made from outside through a lapped top sheet (the
//! skin) into the edge of a web standing under it, a T joint welded blind ([`stake_weld`]). The
//! path is drawn on the skin's outer face, a line or a path of lines and arcs as a seam's
//! ([`crate::weld_seam`]); where it sits, snapped onto the web's middle, is a [`stake_site`].
//!
//! SolidCraft's own, as the spot, the seam and the fusion weld are: every body comes from the
//! kernel interface; this module adds the weld's rules and sizes. The commands and their storage
//! come with #15131.
//!
//! # The geometry
//!
//! A fusion weld's cross-section ([`crate::weld_fusion`]): the cap `0.08 w` off the skin's outer
//! face, and the fusion zone, straight or tapered, from that face through the skin and `depth` into
//! the web (a stake weld never melts through, so it has no root). Where the zone is wider than the
//! web it is cut to the metal: the zone is two bodies, its part in the skin and its part in the
//! web, each the zone met with a copy of that part. Display only: the parts are never changed, and
//! nothing here is volume or mass.
//!
//! # Where it sits
//!
//! The line is snapped onto the web's middle (Q146): moved sideways, an arc to the radius the web
//! turns at. Moved by more than the weld tolerance ([`WELD_TOLERANCE`], Q135), the site says how
//! far; it refuses only when no web lies under the weld's width, and names how far the nearest web
//! is.

use std::f64::consts::FRAC_PI_2;

use solidcraft_kernel::profiles;
use solidcraft_kernel::{Body, BoolOp, KernelError, affine};

use crate::weld_fusion::{FusionRefusal, FusionShape, FusionWeld, fusion_weld};
use crate::weld_seam::{Laid, SeamNotice, SeamPiece, SeamRefusal, add, corners, lay, scale};
use crate::weld_spot::{SpotPiece, SpotSide, SpotZone, positive};

/// The weld tolerance a new stake weld site starts with (Q135, model units: mm): a line off the
/// web's middle by less is snapped without a word.
pub const WELD_TOLERANCE: f64 = 0.1;

/// Why a stake weld is refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum StakeRefusal {
    /// Input the weld cannot take: a width, depth, thickness, point, normal or tolerance out of
    /// range, or a picked face the skin does not have or that is not flat.
    #[error("{0}")]
    Invalid(&'static str),
    /// The path is refused, in the seam's words.
    #[error(transparent)]
    Path(#[from] SeamRefusal),
    /// The weld's own bodies are refused, in the fusion weld's words.
    #[error(transparent)]
    Fusion(#[from] FusionRefusal),
    /// Piece `piece` of the path leaves the picked face.
    #[error("piece {piece} of the stake weld leaves the picked face of the top sheet: draw it on that face")]
    OffFace { piece: usize },
    /// The top sheet is not as thick under piece `piece` as at the path's start.
    #[error("the top sheet is {thickness} thick under piece {piece} of the stake weld, not {start} as at its start: draw it where the sheet is even")]
    SkinUneven { piece: usize, thickness: f64, start: f64 },
    /// Piece `piece` runs too near the edge of the picked face for the weld's width. `fits` is the
    /// widest weld that fits.
    #[error("piece {piece} of the stake weld runs past the edge of the top sheet's face; at most {fits} wide fits")]
    PastFaceEdge { piece: usize, fits: f64 },
    /// No web lies under the weld's width at piece `piece`. `nearest` is how far the nearest web's
    /// middle lies from the line, sideways, if a web lies to either side at all.
    #[error("no web under the stake weld at piece {piece}{}", nearest_text(.nearest))]
    NoWebUnder { piece: usize, nearest: Option<f64> },
    /// The web's middle runs off the path: under piece `piece` it lies `by` further sideways than
    /// at the path's start.
    #[error("the web runs {by} off the stake weld's path at piece {piece}: draw the path along the web")]
    WebNotAlong { piece: usize, by: f64 },
    /// The web stands `gap` away from the top sheet under piece `piece`.
    #[error("the web stands {gap} under the top sheet at piece {piece}: close the gap to the weld tolerance")]
    WebApart { piece: usize, gap: f64 },
    /// The path turns a corner at joint `joint` and the web's middle lies `by` off the line: a
    /// corner is not moved.
    #[error("the web's middle lies {by} off the stake weld's line, which turns a corner at joint {joint}: draw the line on the web's middle")]
    SnapAtCorner { joint: usize, by: f64 },
    /// A kernel call failed.
    #[error(transparent)]
    Kernel(#[from] KernelError),
    /// A profile call the kernel refused, in its words.
    #[error(transparent)]
    Profile(#[from] profiles::ProfileRefused),
    /// A placement call refused its numbers, in the kernel's words.
    #[error(transparent)]
    Placement(#[from] affine::AffineRefused),
}

/// The end of [`StakeRefusal::NoWebUnder`]'s message: the fix.
fn nearest_text(nearest: &Option<f64>) -> String {
    match nearest {
        Some(d) => format!("; the nearest web's middle is {d} away: move the line onto it"),
        None => String::from(": draw it over the web"),
    }
}

type Result<T> = std::result::Result<T, StakeRefusal>;

fn bad<T>(why: &'static str) -> Result<T> {
    Err(StakeRefusal::Invalid(why))
}

/// One stake weld.
#[derive(Debug, Clone, PartialEq)]
pub struct StakeWeld {
    /// Where the path starts: on the skin's underside, over the web.
    pub start: [f64; 3],
    /// The path, piece by piece, on the skin's underside.
    pub path: Vec<SeamPiece>,
    /// The skin's normal, from its outer face into the web (need not be unit).
    pub normal: [f64; 3],
    /// The skin's thickness.
    pub skin: f64,
    /// How deep the weld reaches into the web; a new weld starts at the skin's thickness (Q145).
    pub depth: f64,
    /// The weld's width at the skin's outer face.
    pub width: f64,
    pub shape: FusionShape,
}

/// A stake weld's bodies: the cap, the zone's part in the skin, its part in the web; and what the
/// user should know about its path.
#[derive(Debug)]
pub struct Stake {
    pub pieces: Vec<SpotPiece>,
    pub notices: Vec<SeamNotice>,
}

/// The bodies of `weld` on `skin` and `web`, which are read, never changed.
///
/// # Errors
/// [`StakeRefusal::Invalid`] for a thickness, depth or width that is not finite and positive, and
/// a zone that meets no metal of the skin or the web; [`StakeRefusal::Fusion`] for what the fusion
/// weld refuses (the path, an arc tighter than the weld, sizes the model cannot hold). The errors
/// of the kernel calls otherwise.
pub fn stake_weld(weld: &StakeWeld, skin: &Body, web: &Body) -> Result<Stake> {
    if !(positive(weld.skin) && positive(weld.depth) && positive(weld.width)) {
        return bad("a stake weld skin thickness, depth or width that is not finite and positive");
    }
    // the fusion weld's own bodies, its second sheet twice the depth and the zone half through it:
    // no root
    let fusion = fusion_weld(&FusionWeld {
        start: weld.start,
        path: weld.path.clone(),
        normal: weld.normal,
        thicknesses: vec![weld.skin, 2.0 * weld.depth],
        width: weld.width,
        shape: weld.shape,
        depth: 0.5,
    })?;
    let mut pieces = Vec::new();
    for piece in fusion.pieces {
        if piece.zone != SpotZone::Inside {
            pieces.push(piece);
            continue;
        }
        for (part, side) in [(skin, SpotSide::Lower), (web, SpotSide::Upper)] {
            let Some(body) = solidcraft_kernel::boolean(&piece.body, part, BoolOp::Intersect)? else {
                return bad("a stake weld zone that meets no metal of the top sheet or the web");
            };
            pieces.push(SpotPiece { body, zone: SpotZone::Inside, side });
        }
    }
    Ok(Stake { pieces, notices: fusion.notices })
}

/// Where a stake weld sits: what [`StakeWeld`] needs but its depth, width and shape, and how far
/// the line was moved.
#[derive(Debug, Clone, PartialEq)]
pub struct StakeSite {
    /// Where the path starts, on the skin's underside over the web's middle.
    pub start: [f64; 3],
    /// The path snapped onto the web's middle, on the skin's underside.
    pub path: Vec<SeamPiece>,
    /// The skin's unit normal, from its picked face into the web.
    pub normal: [f64; 3],
    /// The skin's thickness.
    pub skin: f64,
    /// How far the line was moved onto the web's middle, when it was moved by more than the weld
    /// tolerance: the notice for the user.
    pub moved: Option<f64>,
}

/// The site of a stake weld of `width` drawn from `start` along `path` on record face `face` of
/// `skin`, over `web`. Both are read, never changed.
///
/// The path is laid on the picked face, about its inward normal. Under each piece, at a quarter,
/// half and three quarters of its length, the skin must be as thick as at the start, the path must
/// keep half the width from the face's edges, and the web is measured sideways, half the depth
/// into it (the skin's thickness, as a new weld's): its middle must lie as far off the line all
/// along, to the weld `tolerance`. The line is moved there, and the web must touch the skin under
/// it, to the tolerance. A path that turns a corner is moved only within the tolerance: beyond it,
/// refused.
///
/// # Errors
/// [`StakeRefusal::Invalid`] for a face the skin does not have or that is not flat, a start or path
/// point that is not finite, a width or tolerance that is not finite and positive;
/// [`StakeRefusal::Path`] for a path the seam refuses; the other [`StakeRefusal`]s where the parts
/// do not take the weld. The errors of the kernel calls otherwise.
pub fn stake_site(skin: &Body, face: usize, web: &Body, start: [f64; 3], path: &[SeamPiece], width: f64, tolerance: f64) -> Result<StakeSite> {
    if face >= skin.face_count() {
        return bad("a picked face the top sheet does not have");
    }
    if !(positive(width) && positive(tolerance)) {
        return bad("a stake weld width or weld tolerance that is not finite and positive");
    }
    let Some(out) = skin.face_plane_normal(face)? else {
        return bad("a picked face that is not flat");
    };
    let normal = out.value.map(|v| -v);
    let tol = profiles::zone_tolerance()?;
    let laid = lay(start, path, normal, tol)?;
    let quarter = affine::rotation([0.0; 3], normal, FRAC_PI_2)?;
    let sideways = |t: [f64; 3]| affine::apply_point(&quarter, t);

    // under every piece: the skin's thickness, the web's middle sideways
    let mut thick: Option<f64> = None;
    let mut off: Option<f64> = None;
    for (i, piece) in laid.iter().enumerate() {
        for k in [0.25, 0.5, 0.75] {
            let (p, s) = sample(piece, k, &sideways)?;
            let (t, under) = through(skin, face, p, normal, i)?;
            let start_t = *thick.get_or_insert(t);
            if (t - start_t).abs() > tol {
                return Err(StakeRefusal::SkinUneven { piece: i, thickness: t, start: start_t });
            }
            let middle = web_middle(web, add(under, scale(normal, t / 2.0)), s, width, i)?;
            let first = *off.get_or_insert(middle);
            if (middle - first).abs() > tolerance {
                return Err(StakeRefusal::WebNotAlong { piece: i, by: (middle - first).abs() });
            }
        }
    }
    let (Some(skin_t), Some(by)) = (thick, off) else {
        return bad("a stake weld with no path");
    };
    let turns = corners(&laid, normal, tol / width)?;
    let by = match turns.first() {
        Some(SeamNotice::Corner { .. }) if by.abs() <= tolerance => 0.0,
        Some(SeamNotice::Corner { joint, .. }) => return Err(StakeRefusal::SnapAtCorner { joint: *joint, by: by.abs() }),
        None => by,
    };

    // the path moved onto the web's middle, then down to the skin's underside
    let down = scale(normal, skin_t);
    let moved = |p: [f64; 3], t: [f64; 3]| add(add(p, scale(sideways(t), by)), down);
    let (a0, _) = laid.first().map(Laid::ends).ok_or(StakeRefusal::Invalid("a stake weld with no path"))?;
    let (t0, _) = laid.first().map(Laid::tangents).ok_or(StakeRefusal::Invalid("a stake weld with no path"))?;
    let new_start = moved(a0, t0);
    let mut new_path = Vec::new();
    for (piece, drawn) in laid.iter().zip(path) {
        let (_, b) = piece.ends();
        let (_, t1) = piece.tangents();
        new_path.push(match *drawn {
            SeamPiece::Line { .. } => SeamPiece::Line { to: moved(b, t1) },
            SeamPiece::Arc { center, turn, .. } => SeamPiece::Arc { center: add(center, down), to: moved(b, t1), turn },
        });
    }

    // on the moved line: room on the picked face, and the web touching the skin
    let rim = skin.rim_edges(&[face])?;
    let new_laid = lay(new_start, &new_path, normal, tol)?;
    for (i, piece) in new_laid.iter().enumerate() {
        let (a, b) = piece.ends();
        let mut at: Vec<[f64; 3]> = vec![a, b];
        for k in [0.25, 0.5, 0.75] {
            at.push(sample(piece, k, &sideways)?.0);
        }
        for q in at {
            if !rim.is_empty() {
                let top = add(q, scale(normal, -skin_t));
                let Some(c) = skin.closest_to_point(&[], &rim, top)? else {
                    return bad("a top sheet face whose edges were not measured");
                };
                if 2.0 * c.distance < width {
                    return Err(StakeRefusal::PastFaceEdge { piece: i, fits: 2.0 * c.distance });
                }
            }
            match web.face_hits_by_ray(q, normal)?.first() {
                None => return Err(StakeRefusal::NoWebUnder { piece: i, nearest: None }),
                Some(h) if h.distance > tolerance => return Err(StakeRefusal::WebApart { piece: i, gap: h.distance }),
                Some(_) => {}
            }
        }
    }
    let moved = (by.abs() > tolerance).then_some(by.abs());
    Ok(StakeSite { start: new_start, path: new_path, normal, skin: skin_t, moved })
}

/// The point `k` of the way along `piece`, and the unit direction across it there, to its left
/// about the laying normal.
fn sample(piece: &Laid, k: f64, sideways: &impl Fn([f64; 3]) -> [f64; 3]) -> Result<([f64; 3], [f64; 3])> {
    Ok(match *piece {
        Laid::Line { a, t, length, .. } => (add(a, scale(t, k * length)), sideways(t)),
        Laid::Arc { c, axis, u, radius, sweep, .. } => {
            let turn = affine::rotation([0.0; 3], axis, k * sweep)?;
            let r = affine::apply_point(&turn, u[0]);
            let along = affine::apply_point(&affine::rotation([0.0; 3], axis, FRAC_PI_2)?, r);
            (add(c, scale(r, radius)), sideways(along))
        }
    })
}

/// The skin's thickness at `p` on its picked face `face`, along `normal`, and the point under it
/// on its underside.
fn through(skin: &Body, face: usize, p: [f64; 3], normal: [f64; 3], piece: usize) -> Result<(f64, [f64; 3])> {
    let tol = profiles::zone_tolerance()?;
    match skin.face_hits_by_ray(p, normal)?.as_slice() {
        [near, far, ..] if near.face == face && near.distance <= tol => Ok((far.distance - near.distance, far.point)),
        _ => Err(StakeRefusal::OffFace { piece }),
    }
}

/// How far the web's middle lies from `m` along `s` (signed): the web crossed by the line through
/// `m` along `s`, nearest to `m` first. Refused when no web lies within half the `width` of `m`.
fn web_middle(web: &Body, m: [f64; 3], s: [f64; 3], width: f64, piece: usize) -> Result<f64> {
    let mut at: Vec<f64> = web.face_hits_by_ray(m, scale(s, -1.0))?.iter().map(|h| -h.distance).collect();
    at.extend(web.face_hits_by_ray(m, s)?.iter().map(|h| h.distance));
    at.sort_by(f64::total_cmp);
    if !at.len().is_multiple_of(2) {
        return bad("a web the line across it does not cross in and out");
    }
    // the web's spans along the line, the one nearest m
    let nearest = at
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[lo, hi]| (lo, hi))
        .min_by(|a, b| gap(*a).total_cmp(&gap(*b)))
        .ok_or(StakeRefusal::NoWebUnder { piece, nearest: None })?;
    let middle = (nearest.0 + nearest.1) / 2.0;
    if gap(nearest) > width / 2.0 {
        return Err(StakeRefusal::NoWebUnder { piece, nearest: Some(middle.abs()) });
    }
    Ok(middle)
}

/// How far the span `(lo, hi)` lies from 0: none when it holds 0.
fn gap((lo, hi): (f64, f64)) -> f64 {
    if lo > 0.0 {
        lo
    } else if hi < 0.0 {
        -hi
    } else {
        0.0
    }
}

#[cfg(test)]
#[path = "weld_stake_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
