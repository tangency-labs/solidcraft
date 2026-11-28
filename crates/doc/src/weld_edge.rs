//! Edge welds: the display body of one weld laid over the flush ends of plates standing side by
//! side ([`edge_weld`]), and where it sits on the plates the user picked ([`edge_weld_on`]).
//! Flanged butt and flanged corner joints are edge welds too (Q170; AWS A2.4 gives them the edge
//! weld symbol): the cap sits on the flange tips, the same build.
//!
//! SolidCraft's own, as [`crate::weld_spot`] is: every measurement and every body is a kernel call;
//! this module adds the weld's rules and sizes. The `weld.edge` command and its storage come with
//! #15131.
//!
//! # The geometry
//!
//! The path lies on the plates' end faces, along the joint, a line or a chain of lines and arcs as
//! a seam's ([`crate::weld_seam`]); `normal` points out of the end faces. Across the path the cap
//! spans the whole stack, from `across[0]` to `across[1]`, offsets to the path's left (about the
//! normal). A convex cap is a circular segment rising `height` in its middle (Q40; a new weld opens
//! at a quarter of the stack's width, Q169); a flush one is a mark [`FLUSH_MARK`] thick (Q169, as
//! the groove's seam mark). The section is extruded along a line and turned about an arc's centre;
//! at a corner of the path the section is turned about the corner on its outer side, so the cap
//! wraps round it. Display only: the parts are never changed.
//!
//! # Where it sits
//!
//! [`edge_weld_on`] reads the stack from the plates: each plate's end face must be flat and square
//! to the first one's; a ray across the stack just below the lowest end reads where each plate
//! lies. Ends apart by less than the weld tolerance are flush; by more, the cap stands on the
//! lowest end and fills the step, cut away where it would run into a plate, with a notice, while
//! the step is less than the cap's height; a higher step is refused. A gap between plates is
//! bridged with a notice while it is less than the cap's height (Q135).

use std::f64::consts::FRAC_PI_2;

use solidcore_record::Vec3;
use solidcraft_kernel::curves as curve;
use solidcraft_kernel::profiles::{self, ProfileCurve as Curve};
use solidcraft_kernel::{Body, BoolOp, KernelError, affine};

use crate::weld_seam::{Laid, SeamNotice, SeamPiece, SeamRefusal, add, corners, lay, scale};
use crate::weld_spot::positive;

/// How thick a flush edge weld's mark is (Q169, the groove's seam mark, Q98).
pub const FLUSH_MARK: f64 = 0.2;

/// Why an edge weld is refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EdgeRefusal {
    /// Input the weld cannot take: a size, point, normal or tolerance out of range, or sizes the
    /// model cannot hold.
    #[error("{0}")]
    Invalid(&'static str),
    /// The path is refused, in the seam's words.
    #[error(transparent)]
    Path(#[from] SeamRefusal),
    /// Fewer than two plates.
    #[error("an edge weld joins at least two plates")]
    OnePlate,
    /// The end face of plate `plate` is not flat.
    #[error("the end face of plate {plate} is not flat")]
    EndNotFlat { plate: usize },
    /// The end face of plate `plate` is not square to the first plate's.
    #[error("the end face of plate {plate} is not parallel to the first plate's: the ends must lie side by side")]
    EndNotSquare { plate: usize },
    /// The ray across the stack does not cross plate `plate`: it does not stand beside the others
    /// at the path's start.
    #[error("plate {plate} does not stand beside the others where the edge weld starts")]
    NotAlongside { plate: usize },
    /// The plates' ends lie `step` apart: more than the cap's height can fill.
    #[error("the plates' ends lie {step} apart, more than the cap fills: weld it as a fillet, or trim the plates level")]
    EndsApart { step: f64 },
    /// Two plates stand `gap` apart: more than the cap bridges.
    #[error("two plates stand {gap} apart, more than the cap bridges: close the gap to at most {fits}")]
    GapTooWide { gap: f64, fits: f64 },
    /// An arc of the path turns tighter than the cap reaches across: the cap would cross its centre.
    #[error("arc {piece} of the edge weld turns tighter than the cap is wide")]
    ArcTighterThanCap { piece: usize },
    /// The kernel refused a profile's body, in its words.
    #[error(transparent)]
    Profile(#[from] profiles::ProfileRefused),
    /// A curve call the kernel refused, in its words.
    #[error(transparent)]
    Curve(#[from] curve::CurveRefused),
    /// A kernel call on a body failed.
    #[error(transparent)]
    Kernel(#[from] KernelError),
    /// A placement call refused its numbers, in the kernel's words.
    #[error(transparent)]
    Placement(#[from] affine::AffineRefused),
}

type Result<T> = std::result::Result<T, EdgeRefusal>;

fn bad<T>(why: &'static str) -> Result<T> {
    Err(EdgeRefusal::Invalid(why))
}

/// An edge weld's face.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeFace {
    /// A circular segment rising `height` in its middle (the default, Q40).
    Convex { height: f64 },
    /// Flush with the ends: a mark [`FLUSH_MARK`] thick.
    Flush,
}

/// The cap height a new convex edge weld opens with: a quarter of the stack's width (Q169).
pub fn proposed_height(width: f64) -> f64 {
    width / 4.0
}

/// One edge weld.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeWeld {
    /// Where the path starts, on the plane the cap stands on.
    pub start: [f64; 3],
    /// The path, piece by piece, on that plane.
    pub path: Vec<SeamPiece>,
    /// The ends' normal, out of the plates (need not be unit).
    pub normal: [f64; 3],
    /// Where the cap's toes sit: offsets to the path's left about `normal`, `across[0] <
    /// across[1]`.
    pub across: [f64; 2],
    pub face: EdgeFace,
}

/// Something about an edge weld the user should know, though it builds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeNotice {
    /// The path turns a corner (the seam's notice).
    Path(SeamNotice),
    /// The plates' ends lie `by` apart; the cap fills the step.
    Step { by: f64 },
    /// Two plates stand `by` apart; the cap bridges the gap.
    Gap { by: f64 },
}

/// An edge weld's body, and what the user should know about it.
#[derive(Debug)]
pub struct Edge {
    pub body: Body,
    pub notices: Vec<EdgeNotice>,
}

/// The body of `weld`.
///
/// # Errors
/// [`EdgeRefusal::Invalid`] for a normal, offset or height that is not usable, and sizes the model
/// cannot hold; [`EdgeRefusal::Path`] for the path; [`EdgeRefusal::ArcTighterThanCap`] for an arc the
/// cap would cross the centre of. The errors of the kernel calls otherwise.
pub fn edge_weld(weld: &EdgeWeld) -> Result<Edge> {
    let [lo, hi] = weld.across;
    if !(lo.is_finite() && hi.is_finite() && hi - lo > 0.0) {
        return bad("an edge weld whose toes are not finite or not apart");
    }
    let height = match weld.face {
        EdgeFace::Convex { height } if positive(height) => height,
        EdgeFace::Convex { .. } => return bad("an edge weld cap height that is not finite and positive"),
        EdgeFace::Flush => FLUSH_MARK,
    };
    if weld.normal == [0.0; 3] || !weld.normal.iter().all(|v| v.is_finite()) {
        return bad("an edge weld normal that is zero or not finite");
    }
    let tol = profiles::zone_tolerance()?;
    let [n, _] = affine::orthonormal_axes(weld.normal, [1.0, 0.0, 0.0])?;
    let laid = lay(weld.start, &weld.path, n, tol)?;
    let reach = laid.iter().map(Laid::reach).fold(0.0_f64, f64::max) + lo.abs().max(hi.abs()) + height;
    if !reach.is_finite() || reach > profiles::MAX_COORD {
        return bad("an edge weld so large that it passes the model's size limit");
    }
    let quarter = affine::rotation([0.0; 3], n, FRAC_PI_2)?;
    let section = Section { n, lo, hi, height, convex: matches!(weld.face, EdgeFace::Convex { .. }), tol };

    let mut bodies = Vec::new();
    for (i, piece) in laid.iter().enumerate() {
        let (a, _) = piece.ends();
        let (t0, _) = piece.tangents();
        let Some(profile) = section.part_at(a, affine::apply_point(&quarter, t0), Part::Whole)? else {
            return bad("an edge weld cap thinner than the zone tolerance");
        };
        bodies.push(match *piece {
            Laid::Line { t, length, .. } => profiles::extrude_curves(&profile, t, length, true)?,
            Laid::Arc { c, axis, radius, sweep, .. } => {
                // the centre lies to the left when turning left: the cap must not reach it
                let reaches = if axis == n { hi } else { -lo };
                if reaches >= radius - tol {
                    return Err(EdgeRefusal::ArcTighterThanCap { piece: i });
                }
                profiles::revolve_curves(&profile, c, axis, sweep, true, None)?
            }
        });
    }
    // at each corner, the section turned about the corner on its outer side
    let width = hi - lo;
    let turns = corners(&laid, n, tol / width)?;
    for notice in &turns {
        let SeamNotice::Corner { joint, angle } = *notice;
        let (_, b) = laid[joint - 1].ends();
        let (_, t1) = laid[joint - 1].tangents();
        // turning left (about n) the outer side is the right one, at negative offsets
        let (part, axis) = if angle > 0.0 { (Part::Right, n) } else { (Part::Left, scale(n, -1.0)) };
        if let Some(profile) = section.part_at(b, affine::apply_point(&quarter, t1), part)? {
            bodies.push(profiles::revolve_curves(&profile, b, axis, angle.abs(), true, None)?);
        }
    }
    let body = if bodies.len() == 1 { bodies.remove(0) } else { solidcraft_kernel::union_all(&bodies.iter().collect::<Vec<_>>())? };
    Ok(Edge { body, notices: turns.into_iter().map(EdgeNotice::Path).collect() })
}

/// The cap's section: its toes at offsets `lo` and `hi`, `height` high, convex or a flat mark.
struct Section {
    n: [f64; 3],
    lo: f64,
    hi: f64,
    height: f64,
    convex: bool,
    tol: f64,
}

/// Which part of the section a profile takes: all of it, or its part on one side of the path.
#[derive(Clone, Copy, PartialEq)]
enum Part {
    Whole,
    /// At offsets from 0 up (the path's left).
    Left,
    /// At offsets up to 0 (the path's right).
    Right,
}

impl Section {
    /// The closed profile of `part` of the section at `at`, `s` the unit left of the path there;
    /// `None` where that side holds none of it.
    fn part_at(&self, at: [f64; 3], s: [f64; 3], part: Part) -> Result<Option<Vec<Curve>>> {
        let p = |o: f64, z: f64| add(add(at, scale(s, o)), scale(self.n, z));
        let (lo, hi) = match part {
            Part::Whole => (self.lo, self.hi),
            Part::Left => (self.lo.max(0.0), self.hi),
            Part::Right => (self.lo, self.hi.min(0.0)),
        };
        if hi - lo <= self.tol {
            return Ok(None);
        }
        if !self.convex {
            return Ok(Some(vec![
                line(p(lo, 0.0), p(hi, 0.0)),
                line(p(hi, 0.0), p(hi, self.height)),
                line(p(hi, self.height), p(lo, self.height)),
                line(p(lo, self.height), p(lo, 0.0)),
            ]));
        }
        let arc = curve::arc_through([p(self.lo, 0.0), p((self.lo + self.hi) / 2.0, self.height), p(self.hi, 0.0)])?;
        let whole = match part {
            Part::Whole => true,
            Part::Left => self.lo >= 0.0,
            Part::Right => self.hi <= 0.0,
        };
        if whole {
            let mut curves = profiles::curve_pieces(&arc, [0.0, 1.0])?;
            curves.push(line(p(self.hi, 0.0), p(self.lo, 0.0)));
            return Ok(Some(curves));
        }
        // the arc cut where the path's line, standing up along n, meets it
        let Some(cut) = curve::closest_to_ray(&arc, p(0.0, 0.0), self.n, self.tol)?.filter(|c| c.distance <= self.tol) else {
            return bad("an edge weld cap not met over its path");
        };
        let top = cut.point;
        let mut curves = Vec::new();
        if part == Part::Right {
            curves.extend(profiles::curve_pieces(&arc, [0.0, cut.parameter])?);
            curves.extend([line(top, p(0.0, 0.0)), line(p(0.0, 0.0), p(lo, 0.0))]);
        } else {
            curves.extend(profiles::curve_pieces(&arc, [cut.parameter, 1.0])?);
            curves.extend([line(p(hi, 0.0), p(0.0, 0.0)), line(p(0.0, 0.0), top)]);
        }
        Ok(Some(curves))
    }
}

/// One plate of an edge weld and the record face that is its end.
#[derive(Debug, Clone, Copy)]
pub struct EdgePlate<'a> {
    pub body: &'a Body,
    pub end: usize,
}

/// The edge weld along `path` from `start` on the end of `plates[0]`, over every plate listed (the
/// plates the user picked, by an edge or a face of each, Q171), with `face`; `None` opens a convex
/// cap at [`proposed_height`]. `tolerance` is the weld tolerance (Q135). The plates are read, never
/// changed.
///
/// shortcut: the stack is read at the path's start only (one ray across); a stack whose widths or
/// gaps change along the path is not checked. Upgrade with a ray at every joint, as the seam's site
/// checks every joint.
///
/// # Errors
/// [`EdgeRefusal::OnePlate`], [`EdgeRefusal::EndNotFlat`], [`EdgeRefusal::EndNotSquare`],
/// [`EdgeRefusal::NotAlongside`], [`EdgeRefusal::EndsApart`] and [`EdgeRefusal::GapTooWide`] where the
/// plates do not take the weld; those of [`edge_weld`] for the weld itself.
pub fn edge_weld_on(plates: &[EdgePlate], start: [f64; 3], path: &[SeamPiece], face: Option<EdgeFace>, tolerance: f64) -> Result<Edge> {
    if plates.len() < 2 {
        return Err(EdgeRefusal::OnePlate);
    }
    if !positive(tolerance) {
        return bad("a weld tolerance that is not finite and positive");
    }
    let tol = profiles::zone_tolerance()?;
    let Some(n) = plates[0].body.face_plane_normal(plates[0].end)? else {
        return Err(EdgeRefusal::EndNotFlat { plate: 0 });
    };
    let n = n.value;
    let origin = vec3(start);
    // each end's level along n from the start
    let mut levels = Vec::new();
    for (i, p) in plates.iter().enumerate() {
        if p.body.face_plane_normal(p.end)?.is_none() {
            return Err(EdgeRefusal::EndNotFlat { plate: i });
        }
        let Some(level) = p.body.face_square_level(p.end, vec3(n), origin)? else {
            return Err(EdgeRefusal::EndNotSquare { plate: i });
        };
        levels.push(level.level);
    }
    let top = levels.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let low = levels.iter().copied().fold(f64::INFINITY, f64::min);
    let step = top - low;

    // the stack across the path, just below the lowest end
    let laid = lay(start, path, n, tol)?;
    let quarter = affine::rotation([0.0; 3], n, FRAC_PI_2)?;
    let s = affine::apply_point(&quarter, laid[0].tangents().0);
    let q = add(start, scale(n, low - tolerance));
    // one ray along the stack's left, from beyond every plate
    let mut far = 1.0;
    for p in plates {
        let b = p.body.exact_bounds()?;
        far += (b.max.x - b.min.x) + (b.max.y - b.min.y) + (b.max.z - b.min.z);
    }
    let from = add(q, scale(s, -far));
    let mut spans = Vec::new();
    for (i, p) in plates.iter().enumerate() {
        let offsets: Vec<f64> = p.body.face_hits_by_ray(from, s)?.iter().map(|h| h.distance - far).collect();
        let lo = offsets.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = offsets.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if !(hi - lo > tol) {
            return Err(EdgeRefusal::NotAlongside { plate: i });
        }
        spans.push((lo, hi));
    }
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let across = [spans[0].0, spans.iter().map(|s| s.1).fold(f64::NEG_INFINITY, f64::max)];
    let face = face.unwrap_or(EdgeFace::Convex { height: proposed_height(across[1] - across[0]) });
    let fills = match face {
        EdgeFace::Convex { height } => height,
        EdgeFace::Flush => tolerance,
    };

    let mut notices = Vec::new();
    for pair in spans.windows(2) {
        let gap = pair[1].0 - pair[0].1;
        if gap > tolerance {
            if gap >= fills {
                return Err(EdgeRefusal::GapTooWide { gap, fits: fills });
            }
            notices.push(EdgeNotice::Gap { by: gap });
        }
    }
    let base = if step <= tolerance {
        top
    } else if step < fills {
        notices.push(EdgeNotice::Step { by: step });
        low
    } else {
        return Err(EdgeRefusal::EndsApart { step });
    };

    let lift = scale(n, base);
    let weld = EdgeWeld { start: add(start, lift), path: path.iter().map(|p| moved(p, lift)).collect(), normal: n, across, face };
    let mut edge = edge_weld(&weld)?;
    // the cap stands on the lowest end: cut away where it runs into a higher plate
    if base < top {
        for (p, &level) in plates.iter().zip(&levels) {
            if level - base > tolerance {
                edge.body =
                    solidcraft_kernel::boolean(&edge.body, p.body, BoolOp::Cut)?.ok_or(EdgeRefusal::Invalid("an edge weld cut away whole"))?;
            }
        }
    }
    edge.notices.extend(notices);
    Ok(edge)
}

/// `piece` moved by `by`.
fn moved(piece: &SeamPiece, by: [f64; 3]) -> SeamPiece {
    match *piece {
        SeamPiece::Line { to } => SeamPiece::Line { to: add(to, by) },
        SeamPiece::Arc { center, to, turn } => SeamPiece::Arc { center: add(center, by), to: add(to, by), turn },
    }
}

fn vec3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn line(start: [f64; 3], end: [f64; 3]) -> Curve {
    Curve::Line { start, end }
}

#[cfg(test)]
#[path = "weld_edge_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
