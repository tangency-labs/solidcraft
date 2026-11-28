//! Fusion spot and fusion seam welds on lapped sheets: arc, laser or electron beam welds made
//! through the top sheet into the one under it ([`fusion_weld`]). A fusion spot is one point; a
//! fusion seam runs along a path of lines and arcs, as a resistance seam does
//! ([`crate::weld_seam`]). Where they sit is a spot site ([`crate::weld_spot::spot_site`]) of the
//! weld's width, or a fusion seam site ([`fusion_seam_site`]).
//!
//! SolidCraft's own, as the spot and the seam are: every body comes from the kernel interface; this module adds the
//! weld's rules and sizes. The commands and their storage come with #15131.
//!
//! # The geometry
//!
//! Two sheets: the weld is made from the first sheet's outer face (the picked one), through the
//! first sheet and `depth` of the second (a fraction of its thickness; at 1 it melts through).
//! `normal` points from the first sheet into the second. Of the weld's width `w` at the top face:
//!
//! - **the cap** stands `0.08 w` off the top face, a sphere's cap `w` across;
//! - **the fusion zone** runs from the top face down to its depth: [`FusionShape::Straight`]
//!   `0.7 w` across all the way, [`FusionShape::Tapered`] from `w` at the top to `0.35 w` at its
//!   bottom (a laser or electron beam's keyhole);
//! - **the root**, only when it melts through, stands `0.05 w` off the second sheet's outer face,
//!   `1.2` times the zone's bottom width across.
//!
//! These are display proportions, as spot look A's are. A fusion seam is the same cross-section
//! along its path: extruded along a line, turned about an arc's centre, and the spot's own bodies
//! at every joint, each kind of body united into one, so a corner comes out round. Display only:
//! the parts are never changed, and nothing here is volume or mass.

use std::f64::consts::{FRAC_PI_2, TAU};

use solidcore_record::brep::Curve as RecordCurve;
use solidcraft_kernel::curves as curve;
use solidcraft_kernel::profiles::{self, ProfileCurve as Curve};
use solidcraft_kernel::{Body, KernelError, affine};

use crate::weld_seam::{Laid, SeamNotice, SeamPiece, SeamRefusal, SeamSite, add, corners, lay, scale, site};
use crate::weld_spot::{SpotPiece, SpotSide, SpotZone, check_sheets, positive};

/// Why a fusion weld is refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FusionRefusal {
    /// Input the weld cannot take: a width, depth, point, normal or stack out of range, or sizes
    /// the model cannot hold.
    #[error("{0}")]
    Invalid(&'static str),
    /// The seam's path is refused, in the seam's words.
    #[error(transparent)]
    Path(#[from] SeamRefusal),
    /// Arc `piece` of the path turns tighter than the weld is wide. `fits` is the largest width
    /// it takes.
    #[error("arc {piece} of the fusion seam turns tighter than the seam is wide; at most {fits} fits")]
    ArcTighterThanSeam { piece: usize, fits: f64 },
    /// A profile call the kernel refused, in the kernel's words.
    #[error(transparent)]
    Profile(#[from] profiles::ProfileRefused),
    /// A kernel call failed.
    #[error(transparent)]
    Kernel(#[from] KernelError),
    /// A placement call refused its numbers, in the kernel's words.
    #[error(transparent)]
    Placement(#[from] affine::AffineRefused),
    /// A curve call refused, in the kernel's words.
    #[error(transparent)]
    Curve(#[from] curve::CurveRefused),
}

type Result<T> = std::result::Result<T, FusionRefusal>;

fn bad<T>(why: &'static str) -> Result<T> {
    Err(FusionRefusal::Invalid(why))
}

/// The fusion zone's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FusionShape {
    /// As wide all the way down: an arc weld's.
    Straight,
    /// Narrowing with depth: a laser or electron beam weld's keyhole.
    Tapered,
}

impl FusionShape {
    /// The shape a weld tool starts with: tapered for a beam process (laser, electron beam),
    /// straight for arc.
    #[must_use]
    pub const fn of_process(beam: bool) -> Self {
        if beam { Self::Tapered } else { Self::Straight }
    }
}

/// How deep a new fusion weld reaches into the second sheet, as a fraction of its thickness.
pub const DEPTH: f64 = 0.5;

/// One fusion weld: a spot (no path) or a seam.
#[derive(Debug, Clone, PartialEq)]
pub struct FusionWeld {
    /// The spot, or where the seam's path starts: on the faying face between the two sheets.
    pub start: [f64; 3],
    /// The seam's path, piece by piece, on that face; empty for a spot.
    pub path: Vec<SeamPiece>,
    /// That face's normal, from the first sheet (the one the weld is made from) into the second
    /// (need not be unit).
    pub normal: [f64; 3],
    /// The two sheets' thicknesses, the first one first.
    pub thicknesses: Vec<f64>,
    /// The weld's width at the top face.
    pub width: f64,
    pub shape: FusionShape,
    /// How deep it reaches into the second sheet, as a fraction of its thickness: `0 < depth <=
    /// 1`; at 1 it melts through.
    pub depth: f64,
}

/// A fusion weld's bodies, and what the user should know about its path.
#[derive(Debug)]
pub struct Fusion {
    pub pieces: Vec<SpotPiece>,
    pub notices: Vec<SeamNotice>,
}

/// The weld's sizes along the line through it, from the first sheet's outer face (`z = -t1`)
/// through the faying face (`z = 0`) to the second sheet's outer face (`z = t2`).
struct Sizes {
    t1: f64,
    t2: f64,
    /// The cap: its radius and height.
    cap: (f64, f64),
    /// The zone: its radius at the top face and at its bottom, and the bottom's height.
    zone: (f64, f64, f64),
    /// The root, if it melts through: its radius and height.
    root: Option<(f64, f64)>,
}

/// The bodies of `weld`.
///
/// # Errors
/// [`FusionRefusal::Invalid`] for a start, normal or path point that is not finite, a zero normal,
/// not exactly two sheets, a thickness or width that is not finite and positive, a depth outside
/// `0 < depth <= 1`, and sizes the model cannot hold; [`FusionRefusal::Path`] for a path the seam
/// refuses; [`FusionRefusal::ArcTighterThanSeam`] for an arc tighter than the weld. The errors of
/// the ops calls otherwise.
pub fn fusion_weld(weld: &FusionWeld) -> Result<Fusion> {
    let (t1, t2) = check_sheets(&weld.thicknesses)
        .map_err(|_| FusionRefusal::Invalid("fewer than two sheets, or a thickness that is not finite and positive"))?;
    if weld.thicknesses.len() != 2 {
        return bad("a fusion weld on other than two sheets");
    }
    if !positive(weld.width) {
        return bad("a fusion weld width that is not finite and positive");
    }
    if !(weld.depth > 0.0 && weld.depth <= 1.0) {
        return bad("a fusion weld depth outside 0 < depth <= 1");
    }
    if weld.normal == [0.0; 3] || !weld.normal.iter().chain(&weld.start).all(|v| v.is_finite()) {
        return bad("a fusion weld normal that is zero, or a start or normal that is not finite");
    }
    let tol = profiles::zone_tolerance()?;
    let w = weld.width;
    let (top, bottom) = match weld.shape {
        FusionShape::Straight => (0.35 * w, 0.35 * w),
        FusionShape::Tapered => (0.5 * w, 0.175 * w),
    };
    let through = weld.depth >= 1.0;
    let sizes = Sizes { t1, t2, cap: (0.5 * w, 0.08 * w), zone: (top, bottom, weld.depth * t2), root: through.then_some((1.2 * bottom, 0.05 * w)) };
    if [sizes.cap.1, bottom, 0.05 * w].iter().any(|&l| l < tol) {
        return bad("a fusion weld so small that a piece of it is thinner than the zone tolerance");
    }
    let [n, x] = affine::orthonormal_axes(weld.normal, [1.0, 0.0, 0.0])?;
    let laid = if weld.path.is_empty() { Vec::new() } else { lay(weld.start, &weld.path, n, tol)? };
    let reach = laid.iter().map(Laid::reach).fold(weld.start.iter().fold(0.0_f64, |m, c| m.max(c.abs())), f64::max) + w + t1 + t2;
    if !reach.is_finite() || reach > profiles::MAX_COORD {
        return bad("a fusion weld so large that it passes the model's size limit");
    }
    let widest = sizes.cap.0.max(top).max(sizes.root.map_or(0.0, |r| r.0));
    let quarter = affine::rotation([0.0; 3], n, FRAC_PI_2)?;
    let joints: Vec<[f64; 3]> = std::iter::once(weld.start).chain(laid.iter().map(|p| p.ends().1)).collect();

    let mut pieces = Vec::new();
    for (kind, zone, side) in [
        (Kind::Cap, SpotZone::Core, SpotSide::Lower),
        (Kind::Zone, SpotZone::Inside, SpotSide::Through),
        (Kind::Root, SpotZone::Core, SpotSide::Upper),
    ] {
        if kind == Kind::Root && sizes.root.is_none() {
            continue;
        }
        let mut bodies = Vec::new();
        for &at in &joints {
            let plane = Plane { origin: at, side: x, up: n };
            bodies.push(profiles::revolve_curves(&sizes.profile(kind, plane, false, tol)?, at, n, TAU, true, None)?);
        }
        for (i, piece) in laid.iter().enumerate() {
            let (a, _) = piece.ends();
            let (t0, _) = piece.tangents();
            let plane = Plane { origin: a, side: affine::apply_point(&quarter, t0), up: n };
            let across = sizes.profile(kind, plane, true, tol)?;
            bodies.push(match *piece {
                Laid::Line { t, length, .. } => profiles::extrude_curves(&across, t, length, true)?,
                Laid::Arc { c, axis, radius, sweep, .. } => {
                    if radius - widest <= tol {
                        return Err(FusionRefusal::ArcTighterThanSeam { piece: i, fits: radius * w / widest });
                    }
                    profiles::revolve_curves(&across, c, axis, sweep, true, None)?
                }
            });
        }
        pieces.push(SpotPiece { body: solidcraft_kernel::union_all(&bodies.iter().collect::<Vec<_>>())?, zone, side });
    }
    let notices = if laid.is_empty() { Vec::new() } else { corners(&laid, n, tol / w)? };
    Ok(Fusion { pieces, notices })
}

/// The site of a fusion seam of `width` drawn from `start` along `path` on record face `face` of
/// sheet `picked`: a seam site ([`crate::weld_seam::seam_site`]) whose arcs must turn wider than
/// this weld's widest part, the cap's `0.5 w` (the resistance seam's edge zone is wider).
///
/// # Errors
/// [`FusionRefusal::Path`] with the seam site's refusal, [`SeamRefusal::ArcTighterThanSeam`] for
/// an arc the weld cannot follow.
pub fn fusion_seam_site(sheets: &[&Body], picked: usize, face: usize, start: [f64; 3], path: &[SeamPiece], width: f64) -> Result<SeamSite> {
    Ok(site(sheets, picked, face, start, path, width, 0.5 * width)?)
}

/// Which body of the weld.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Cap,
    Zone,
    Root,
}

/// Where a profile lies: `origin + side · s + up · z` for the distance `s` across the weld and the
/// height `z` along the normal.
#[derive(Clone, Copy)]
struct Plane {
    origin: [f64; 3],
    side: [f64; 3],
    up: [f64; 3],
}

impl Plane {
    fn at(self, s: f64, z: f64) -> [f64; 3] {
        add(self.origin, add(scale(self.side, s), scale(self.up, z)))
    }
}

impl Sizes {
    /// The closed profile of body `kind` on `plane`: the whole cross-section (`whole`, swept along
    /// the path) or its half from the axis out (`!whole`, turned a whole turn about it).
    fn profile(&self, kind: Kind, plane: Plane, whole: bool, tol: f64) -> Result<Vec<Curve>> {
        let line = |s0: f64, z0: f64, s1: f64, z1: f64| Curve::Line { start: plane.at(s0, z0), end: plane.at(s1, z1) };
        match kind {
            Kind::Zone => {
                let (top, bottom, depth) = self.zone;
                let (zt, zb) = (-self.t1, depth);
                Ok(if whole {
                    vec![line(-top, zt, top, zt), line(top, zt, bottom, zb), line(bottom, zb, -bottom, zb), line(-bottom, zb, -top, zt)]
                } else {
                    vec![line(0.0, zt, top, zt), line(top, zt, bottom, zb), line(bottom, zb, 0.0, zb), line(0.0, zb, 0.0, zt)]
                })
            }
            Kind::Cap => self.dome(plane, self.cap, -self.t1, -1.0, whole, tol),
            Kind::Root => {
                let Some(root) = self.root else {
                    return bad("a root on a fusion weld that does not melt through");
                };
                self.dome(plane, root, self.t2, 1.0, whole, tol)
            }
        }
    }

    /// A sphere's cap of radius `r` and height `h` standing off the face at `face` along `sense`:
    /// the face line, and the arc through its rims and its top ([`curve::arc_through`]), the whole
    /// of it or from the axis out.
    fn dome(&self, plane: Plane, (r, h): (f64, f64), face: f64, sense: f64, whole: bool, tol: f64) -> Result<Vec<Curve>> {
        let arc = curve::arc_through([plane.at(-r, face), plane.at(0.0, face + sense * h), plane.at(r, face)])?;
        let from = if whole { -r } else { 0.0 };
        let (ta, za) = param_at(&arc, plane, from, face, sense, tol)?;
        let (tb, _) = param_at(&arc, plane, r, face, sense, tol)?;
        let mut curves = vec![Curve::Line { start: plane.at(from, face), end: plane.at(r, face) }];
        curves.extend(profiles::curve_pieces(&arc, [ta.min(tb), ta.max(tb)])?);
        if !whole {
            curves.push(Curve::Line { start: za, end: plane.at(0.0, face) });
        }
        Ok(curves)
    }
}

/// The parameter where the arc `arc` crosses the line at `s` across `plane` (from the face at
/// `face`, out along `sense`), and the point there.
fn param_at(arc: &RecordCurve, plane: Plane, s: f64, face: f64, sense: f64, tol: f64) -> Result<(f64, [f64; 3])> {
    let found = curve::closest_to_ray(arc, plane.at(s, face), scale(plane.up, sense), tol)?;
    let Some(p) = found.filter(|p| p.distance <= tol) else {
        return bad("a cap whose height across it was not found");
    };
    Ok((p.parameter, p.point))
}

#[cfg(test)]
#[path = "weld_fusion_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
