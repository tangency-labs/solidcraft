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
//! turns at, a corner to where the moved pieces meet. The snapped path is checked whole, by the
//! kernel's exact distances from it to the parts' faces and edges, never at sample points.
//!
//! # Tolerances
//!
//! Each kind of deviation has its own weld tolerance ([`StakeTolerances`], Q135, Q190; the values
//! and their sources in the tracker's weld tolerance table): within it the weld counts as perfect,
//! past it the weld is still built where a sensible one exists, with a [`StakeNotice`] saying what
//! was off and by how much. It is refused only when no sensible weld exists, naming the fix: no web
//! under the weld's width (the nearest web's distance), a web too far down for the zone to reach,
//! a web through the sheet, a path off the picked face or past its edges.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use solidcore_record::{Seg2, Vec2, Vec3};
use solidcraft_kernel::{Body, BoolOp, KernelError, PointClass, affine, develop, profiles};

use crate::weld_fusion::{FusionRefusal, FusionShape, FusionWeld, fusion_weld};
use crate::weld_seam::{Laid, SeamNotice, SeamPiece, SeamRefusal, add, corners, lay, scale};
use crate::weld_spot::{SpotPiece, SpotSide, SpotZone, positive};

/// A stake weld's tolerances, one per kind of deviation (Q135, Q190), in model units (mm): within
/// each the weld counts as perfect, past it the site gives a [`StakeNotice`]. The user can change
/// every value. ISO 13919-1:2019 (the quality levels for beam welds) sets no fit-up limits, so the
/// defaults are proposed values, each with its reason.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StakeTolerances {
    /// The line off the web's middle: proposed 0.1 mm (Q146).
    pub offset: f64,
    /// A gap between the web and the top sheet, as a share of the sheet's thickness: proposed
    /// 10 %, the fit-up rule TWI gives for laser lap welds (guidance, not a standard).
    pub gap_share: f64,
    /// The web reaching up into the top sheet: proposed 0.1 mm.
    pub overlap: f64,
    /// The top sheet uneven, or the web not running whole along the path: proposed 0.1 mm.
    pub drift: f64,
}

impl StakeTolerances {
    /// The defaults a new stake weld starts with.
    pub const DEFAULT: Self = Self { offset: 0.1, gap_share: 0.1, overlap: 0.1, drift: 0.1 };

    fn valid(&self) -> bool {
        positive(self.offset) && positive(self.gap_share) && positive(self.overlap) && positive(self.drift)
    }
}

impl Default for StakeTolerances {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// What was off in parts that still take the stake weld, past its tolerance, and by how much.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StakeNotice {
    /// The line was moved `by` onto the web's middle.
    Moved { by: f64 },
    /// The web stands `gap` under the top sheet at the start; the zone bridges it.
    Gap { gap: f64 },
    /// The web reaches `by` up into the top sheet at the start; the zone is cut to each part's
    /// metal, overlap and all (T199).
    Overlap { by: f64 },
    /// Under piece `piece` the top sheet is not whole and even: a pocket, a step or a hole in its
    /// underside comes `by` nearer its middle than half its thickness.
    SkinUneven { piece: usize, by: f64 },
    /// Under piece `piece` the web does not run whole along the path: it turns off, ends or has a
    /// hole or a gap there, coming `by` nearer the path than at the start.
    WebNotAlong { piece: usize, by: f64 },
}

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
    /// The path starts off the picked face, or, snapped onto the web, starts off the top sheet.
    #[error("the stake weld starts off the picked face of the top sheet: draw it on that face, over the web")]
    OffFace,
    /// Piece `piece` runs too near the edge of the picked face for the weld's width. `fits` is the
    /// widest weld that fits.
    #[error("piece {piece} of the stake weld runs past the edge of the top sheet's face; at most {fits} wide fits")]
    PastFaceEdge { piece: usize, fits: f64 },
    /// No web lies under the weld's width at the path's start. `nearest` is how far the nearest
    /// web's middle lies from the line, sideways, if the line across the start meets a web at all.
    #[error("no web under the stake weld{}", nearest_text(.nearest))]
    NoWebUnder { nearest: Option<f64> },
    /// The web stands `gap` under the top sheet at the path's start, as far as the zone reaches
    /// (a new weld's depth, the sheet's thickness) or further: the zone cannot reach it. (Less is
    /// built, with [`StakeNotice::Gap`].)
    #[error("the web stands {gap} under the top sheet, out of the weld's reach: close the gap to under {reach}")]
    WebApart { gap: f64, reach: f64 },
    /// The web reaches `by` up into the top sheet at the path's start, as far as its outer face or
    /// further: no sheet is left to weld through. (Less is built, with [`StakeNotice::Overlap`].)
    #[error("the web reaches {by} up into the top sheet, through it: trim it to the sheet's underside")]
    WebOverlap { by: f64 },
    /// Snapped onto the web, `by` sideways, the pieces at joint `joint` no longer meet, or one of
    /// them runs backwards.
    #[error(
        "moved {by} onto the web's middle, the stake weld's path no longer meets itself at joint {joint}: draw the line on the web's middle there"
    )]
    SnapAtCorner { joint: usize, by: f64 },
    /// The kernel measured no distance from piece `piece` to the top sheet's or the web's faces.
    #[error("piece {piece} of the stake weld could not be measured against the parts")]
    NotMeasured { piece: usize },
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
    /// What was off past its tolerance, for the user.
    pub notices: Vec<StakeNotice>,
}

/// The site of a stake weld of `width` drawn from `start` along `path` on record face `face` of
/// `skin`, over `web`. Both are read, never changed.
///
/// The path is laid on the picked face, about its inward normal. Across its start, just under the
/// web's point nearest it, the line square to the path is split where it crosses the web's faces,
/// and each stretch is inside the web or not by the kernel's classification of its middle; the
/// web's middle is the middle of the stretch inside it nearest the line. The path is moved there:
/// each piece sideways (an arc to the radius the web turns at), a corner to where the moved pieces
/// meet ([`solidcraft_kernel::develop::crossings`]). A path with a corner that lies within the
/// line's tolerance of the web's middle is kept as drawn.
///
/// The snapped path is then checked whole, by the kernel's exact distance from each piece to the
/// parts' faces and edges, not at sample points: the room to the picked face's edges, holes
/// included; the top sheet whole and even around the path at its mid-thickness (half its
/// thickness from every face); the web whole around the path lowered into it (as far from every
/// face as from its top); and, at the start, the web touching the top sheet. Each against its
/// tolerance in `tolerances`: past it, a [`StakeNotice`].
///
/// # Errors
/// [`StakeRefusal::Invalid`] for a face the skin does not have or that is not flat, a start that
/// is not finite, a width or a tolerance that is not finite and positive; [`StakeRefusal::Path`] for
/// a path the seam refuses; the other [`StakeRefusal`]s where the parts do not take the weld. The
/// errors of the kernel calls otherwise.
pub fn stake_site(
    skin: &Body,
    face: usize,
    web: &Body,
    start: [f64; 3],
    path: &[SeamPiece],
    width: f64,
    tolerances: StakeTolerances,
) -> Result<StakeSite> {
    if face >= skin.face_count() {
        return bad("a picked face the top sheet does not have");
    }
    if !(positive(width) && tolerances.valid()) {
        return bad("a stake weld width or weld tolerance that is not finite and positive");
    }
    if !start.iter().all(|v| v.is_finite()) {
        return bad("a stake weld start that is not finite");
    }
    let Some(out) = skin.face_plane_normal(face)? else {
        return bad("a picked face that is not flat");
    };
    let normal = out.value.map(|v| -v);
    let tol = profiles::zone_tolerance()?;
    let laid = lay(start, path, normal, tol)?;
    let quarter = affine::rotation([0.0; 3], normal, FRAC_PI_2)?;
    let sideways = |t: [f64; 3]| affine::apply_point(&quarter, t);
    let Some((first, _)) = laid.first().map(Laid::tangents) else {
        return bad("a stake weld with no path");
    };

    // the skin's thickness at the start; the web across the start, just under its nearest point
    let (t, under) = through(skin, face, start, normal, tol)?;
    let web_faces: Vec<usize> = (0..web.face_count()).collect();
    let Some(nearest) = web.closest_to_point(&web_faces, &[], under)? else {
        return Err(StakeRefusal::NoWebUnder { nearest: None });
    };
    let level = affine::distance_along(under, normal, nearest.to).max(0.0);
    let probe = add(under, scale(normal, level + t / 2.0));
    let Some((lo, hi)) = web_across(web, probe, sideways(first), tol)?.into_iter().min_by(|a, b| gap(*a).total_cmp(&gap(*b))) else {
        return Err(StakeRefusal::NoWebUnder { nearest: None });
    };
    let middle = (lo + hi) / 2.0;
    if gap((lo, hi)) > width / 2.0 {
        return Err(StakeRefusal::NoWebUnder { nearest: Some(middle.abs()) });
    }

    // the path moved onto the web's middle, then down to the skin's underside
    let turns = corners(&laid, normal, tol / width)?;
    let by = if !turns.is_empty() && middle.abs() <= tolerances.offset { 0.0 } else { middle };
    let joints = snapped_joints(&laid, normal, &turns, by, &sideways, width, tol)?;
    let down = scale(normal, t);
    let Some(&start_joint) = joints.first() else {
        return bad("a stake weld with no path");
    };
    let new_start = add(start_joint, down);
    let mut new_path = Vec::new();
    for (drawn, &joint) in path.iter().zip(joints.iter().skip(1)) {
        let to = add(joint, down);
        new_path.push(match *drawn {
            SeamPiece::Line { .. } => SeamPiece::Line { to },
            SeamPiece::Arc { center, turn, .. } => SeamPiece::Arc { center: add(center, down), to, turn },
        });
    }
    let new_laid = lay(new_start, &new_path, normal, tol).map_err(|_| StakeRefusal::SnapAtCorner { joint: 1, by: by.abs() })?;
    for (i, (was, now)) in laid.iter().zip(&new_laid).enumerate() {
        let backwards = match (was, now) {
            (Laid::Line { t: along, .. }, Laid::Line { a, b, .. }) => affine::distance_along(*a, *along, *b) <= tol,
            (Laid::Arc { sweep: s0, .. }, Laid::Arc { sweep: s1, .. }) => (s0 - s1).abs() > PI,
            _ => true,
        };
        if backwards {
            return Err(StakeRefusal::SnapAtCorner { joint: i.max(1), by: by.abs() });
        }
    }

    // the snapped start on the picked face itself, not on another face of the sheet: with the
    // room to the face's edges checked below along the whole path, the path stays on it
    through(skin, face, add(new_start, scale(normal, -t)), normal, tol)?;

    // the web against the top sheet at the start: touching, apart (the zone reaches a new weld's
    // depth, the sheet's thickness, below it) or reaching into it
    let mut notices = Vec::new();
    let mut apart = 0.0;
    match web.classify_point(Vec3::from(new_start))? {
        PointClass::OnBoundary => {}
        PointClass::Outside => match web.face_hits_by_ray(new_start, normal)?.first() {
            None => return Err(StakeRefusal::NoWebUnder { nearest: None }),
            Some(h) if h.distance >= t - tol => return Err(StakeRefusal::WebApart { gap: h.distance, reach: t }),
            Some(h) => {
                apart = h.distance;
                if h.distance > tolerances.gap_share * t {
                    notices.push(StakeNotice::Gap { gap: h.distance });
                }
            }
        },
        PointClass::Inside => match web.face_hits_by_ray(new_start, scale(normal, -1.0))?.first() {
            Some(h) if h.distance >= t - tol => return Err(StakeRefusal::WebOverlap { by: h.distance }),
            Some(h) if h.distance > tolerances.overlap => notices.push(StakeNotice::Overlap { by: h.distance }),
            _ => {}
        },
    }

    // the snapped path whole: room on the picked face, holes included
    let rim = skin.rim_edges(&[face])?;
    let mut tightest: Option<(f64, usize)> = None;
    for (i, piece) in new_laid.iter().enumerate() {
        if rim.is_empty() {
            break;
        }
        let (helper, edge) = piece.helper(scale(normal, -t), normal, width, tol)?;
        let c = skin.closest_to_edge_of(&[], &rim, &helper, edge)?.ok_or(StakeRefusal::NotMeasured { piece: i })?;
        if tightest.is_none_or(|(room, _)| c.distance < room) {
            tightest = Some((c.distance, i));
        }
    }
    if let Some((room, piece)) = tightest.filter(|(room, _)| 2.0 * room < width) {
        return Err(StakeRefusal::PastFaceEdge { piece, fits: 2.0 * room });
    }

    // the top sheet whole and even around the path at mid-thickness; the web whole around it
    // lowered into the web
    let skin_faces: Vec<usize> = (0..skin.face_count()).collect();
    let lift = scale(normal, -t / 2.0);
    if skin.classify_point(Vec3::from(add(new_start, lift)))? != PointClass::Inside {
        return Err(StakeRefusal::OffFace);
    }
    // the web's own: h into it below its top, as far as its middle's reach
    let h = t.min(hi - lo) / 2.0;
    let mut uneven: Option<(usize, f64)> = None;
    let mut off: Option<(usize, f64)> = None;
    let worst = |slot: &mut Option<(usize, f64)>, i: usize, by: f64| {
        if slot.is_none_or(|(_, was)| by > was) {
            *slot = Some((i, by));
        }
    };
    for (i, piece) in new_laid.iter().enumerate() {
        let (helper, edge) = piece.helper(lift, normal, width, tol)?;
        let c = skin.closest_to_edge_of(&skin_faces, &[], &helper, edge)?.ok_or(StakeRefusal::NotMeasured { piece: i })?;
        if t / 2.0 - c.distance > tolerances.drift {
            worst(&mut uneven, i, t / 2.0 - c.distance);
        }
        let (helper, edge) = piece.helper(scale(normal, apart + h), normal, width, tol)?;
        let c = web.closest_to_edge_of(&web_faces, &[], &helper, edge)?.ok_or(StakeRefusal::NotMeasured { piece: i })?;
        if h - c.distance > tolerances.drift {
            worst(&mut off, i, h - c.distance);
        }
    }
    if by.abs() > tolerances.offset {
        notices.insert(0, StakeNotice::Moved { by: by.abs() });
    }
    notices.extend(uneven.map(|(piece, by)| StakeNotice::SkinUneven { piece, by }));
    notices.extend(off.map(|(piece, by)| StakeNotice::WebNotAlong { piece, by }));
    Ok(StakeSite { start: new_start, path: new_path, normal, skin: t, notices })
}

/// The joints of `laid` moved `by` sideways (left about `normal`), on the face: with the pieces
/// moved alike (a line sideways, an arc to a radius `by` less or more), a joint where the path
/// runs on is moved with them, and a corner goes where the two moved pieces meet: of the kernel's
/// crossings of the two in the face's plane, the one nearest the corner moved sideways.
fn snapped_joints(
    laid: &[Laid],
    normal: [f64; 3],
    turns: &[SeamNotice],
    by: f64,
    sideways: &impl Fn([f64; 3]) -> [f64; 3],
    width: f64,
    tol: f64,
) -> Result<Vec<[f64; 3]>> {
    let moved = |p: [f64; 3], t: [f64; 3]| add(p, scale(sideways(t), by));
    let Some((first, (a0, _))) = laid.first().map(|p| (p.tangents().0, p.ends())) else {
        return bad("a stake weld with no path");
    };
    let mut joints = vec![moved(a0, first)];
    for piece in laid {
        joints.push(moved(piece.ends().1, piece.tangents().1));
    }
    // every arc keeps a radius, corners or not
    for piece in laid {
        moved_radius(piece, normal, by, tol)?;
    }
    if by == 0.0 || turns.is_empty() {
        return Ok(joints);
    }

    // the face's plane: the first tangent, the side, the normal, about the path's start
    let [n, u] = affine::orthonormal_axes(normal, first)?;
    let w = sideways(u);
    let frame: affine::Matrix = [[u[0], u[1], u[2], 0.0], [w[0], w[1], w[2], 0.0], [n[0], n[1], n[2], 0.0], [a0[0], a0[1], a0[2], 1.0]];
    let back = affine::inverse(&frame)?;
    let flat = |p: [f64; 3]| {
        let q = affine::apply_point(&back, p);
        Vec2::new(q[0], q[1])
    };
    // a moved line runs on past its ends by more than the whole path, so a corner's crossing is
    // on it however sharp the corner
    let reach: f64 = laid
        .iter()
        .map(|p| match *p {
            Laid::Line { length, .. } => length,
            Laid::Arc { radius, sweep, .. } => radius * sweep,
        })
        .sum::<f64>()
        * 4.0
        + 4.0 * (width + by.abs());
    let moved_piece = |p: &Laid| -> Result<Seg2> {
        Ok(match *p {
            Laid::Line { a, b, t, length } => {
                let (p, q) = (flat(moved(a, t)), flat(moved(b, t)));
                let k = reach / length;
                Seg2::Line { a: Vec2::new(p.x - (q.x - p.x) * k, p.y - (q.y - p.y) * k), b: Vec2::new(q.x + (q.x - p.x) * k, q.y + (q.y - p.y) * k) }
            }
            Laid::Arc { c, .. } => Seg2::Arc { center: flat(c), radius: moved_radius(p, normal, by, tol)?, start: 0.0, sweep: TAU },
        })
    };
    for turn in turns {
        let SeamNotice::Corner { joint, .. } = *turn;
        let (Some(before), Some(after), Some(&near)) = (laid.get(joint - 1), laid.get(joint), joints.get(joint)) else {
            continue;
        };
        let near = flat(near);
        let far = |p: Vec2| (p.x - near.x).abs().max((p.y - near.y).abs());
        let met =
            develop::crossings(&[moved_piece(before)?], &[moved_piece(after)?]).into_iter().map(|c| c.4).min_by(|p, q| far(*p).total_cmp(&far(*q)));
        let Some(met) = met else {
            return Err(StakeRefusal::SnapAtCorner { joint, by: by.abs() });
        };
        if let Some(j) = joints.get_mut(joint) {
            *j = affine::apply_point(&frame, [met.x, met.y, 0.0]);
        }
    }
    Ok(joints)
}

/// The radius of arc `piece` moved `by` sideways (left about `normal`): left of a left turn is
/// towards the centre. Refused when no radius is left. A line's is none (0).
fn moved_radius(piece: &Laid, normal: [f64; 3], by: f64, tol: f64) -> Result<f64> {
    let Laid::Arc { axis, radius, .. } = *piece else {
        return Ok(0.0);
    };
    let r = if affine::distance_along([0.0; 3], normal, axis) > 0.0 { radius - by } else { radius + by };
    if r <= tol {
        return bad("a stake weld arc that moved onto the web's middle has no radius left");
    }
    Ok(r)
}

/// The skin's thickness at `p` on its picked face `face`, along `normal`, and the point under it
/// on its underside.
fn through(skin: &Body, face: usize, p: [f64; 3], normal: [f64; 3], tol: f64) -> Result<(f64, [f64; 3])> {
    match skin.face_hits_by_ray(p, normal)?.as_slice() {
        [near, far, ..] if near.face == face && near.distance <= tol => Ok((far.distance - near.distance, far.point)),
        _ => Err(StakeRefusal::OffFace),
    }
}

/// The stretches of the line through `m` along unit `s` that lie inside `web`, as signed distances
/// from `m` along `s`: the line split where it meets the web's faces (a point met twice, through
/// an edge or from both sides of `m`, is one), each stretch inside by the kernel's classification
/// of its middle, neighbouring stretches inside joined.
fn web_across(web: &Body, m: [f64; 3], s: [f64; 3], tol: f64) -> Result<Vec<(f64, f64)>> {
    let mut at: Vec<f64> = web.face_hits_by_ray(m, scale(s, -1.0))?.iter().map(|h| -h.distance).collect();
    at.extend(web.face_hits_by_ray(m, s)?.iter().map(|h| h.distance));
    at.sort_by(f64::total_cmp);
    at.dedup_by(|later, earlier| (*later - *earlier).abs() <= tol);
    let middles: Vec<Vec3> = at.windows(2).map(|w| Vec3::from(add(m, scale(s, w.iter().sum::<f64>() / 2.0)))).collect();
    let classes = web.classify_points(&middles)?;
    let mut spans: Vec<(f64, f64)> = Vec::new();
    for (w, class) in at.windows(2).zip(classes) {
        let [lo, hi] = *w else {
            continue;
        };
        if class != PointClass::Inside {
            continue;
        }
        match spans.last_mut() {
            Some(last) if (last.1 - lo).abs() <= tol => last.1 = hi,
            _ => spans.push((lo, hi)),
        }
    }
    Ok(spans)
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
