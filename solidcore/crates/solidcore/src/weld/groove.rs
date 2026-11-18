//! Butt and groove welds: one weld across a joint, filling the groove that is already modelled
//! in the parts.
//!
//! Not upstream: SolidCraft's own (#15132, #18500). The weld is built in two steps: the groove's
//! section, read from the parts ([`GrooveSection`]), and [`fill`], which fills it with the
//! user's options and returns the weld ([`GrooveWeld`]). Nothing here sees or changes a part.
//! For now the section is plain planar geometry the caller passes in (the two walls in the
//! plane square to the joint); reading it from the parts, and the body along the joint, are
//! the next steps of #18500.
//!
//! | Rust              | What it is                                                           |
//! |-------------------|----------------------------------------------------------------------|
//! | [`GrooveSection`] | The groove across the joint: its two walls and the plates' normal    |
//! | [`FillOptions`]   | The contour of each face and the penetration depth `s`               |
//! | [`Contour`]       | Flat, convex (sharp or blended toes) or concave                      |
//! | [`fill`]          | The weld: the groove filled, closed by its faces                     |
//! | [`GrooveWeld`]    | The weld's section, the groove's and the filled depth, its notes     |
//! | [`GrooveNote`]    | What the weld tool tells the user about a weld it built              |
//! | [`WeldFailure`]   | Why a weld is refused (shared by every weld type, `error::weld`)     |
//!
//! The options have no defaults here: the caller (ops) sets them (a flush face, a blend
//! overlap equal to the crest height).
//!
//! # The geometry
//!
//! The weld is generic: it fills whatever groove the walls bound, whatever its shape (V,
//! bevel, Y, U, J, the double-sided X and K, other angles, steps, rounds). The weld type is a
//! name for the symbol, not a recipe.
//!
//! Each wall is the groove face of one part as it cuts the section plane, a chain of lines and
//! arcs. It runs from its *toe* on the face side (where the groove face meets the part's outer
//! face) away from the face. Seen from the face side, `left` is on the left. A wall ends where
//! the groove ends: on the root side's outer face when the groove runs through the plate, or
//! where the two parts touch (both walls then end at the same point, the groove is closed).
//!
//! The *face* closes the weld between the two toes. Where the groove runs through the plate
//! the other side is closed between the two wall ends: flat on a weld from one side (its root,
//! always flush), or by the *second face* of a weld from both sides (a weld symbol above and
//! below the reference line: X, K, double-U, double-J, or any groove the user welds from both
//! sides), which follows the user's contour for that side. Each face is [`Contour::Flat`] (a
//! straight chord), [`Contour::Convex`] (an arc through the toes with its crest the given
//! height off the chord), [`Contour::ConvexBlended`] (the same crest, run out tangentially
//! past the toes) or [`Contour::Concave`] (an arc dipping the given depth below the chord).
//!
//! **Blended toes** (ISO 2553, "toes shall be blended smoothly"): a convex arc of radius `R`
//! between the toes and at each toe a concave arc of radius `r`, tangent to the chord's line
//! at its outer end and to the convex arc at the toe. The crest height is `h`, the blend runs
//! `o` past each toe. With half the chord `a` and `x = a + o`: `R + r = (x² + h²) / 2h`,
//! `R = a (R + r) / x`. `o = 0` is the plain convex face. The blend runs along the chord's line,
//! so it assumes the part's outer faces lie on it (plates in one plane, toes at one height).
//!
//! **Penetration depth `s`.** Where the plates' outer faces are parallel (a butt joint,
//! [`GrooveSection::plate_normal`] given), `s` is measured square to the plates from the face
//! side's outer face of the lower plate (the one whose toe lies deeper), and the weld's bottom
//! runs parallel to the plates: plates of unequal thickness or offset get `s` in the plate
//! whose surface is lower, the other plate more by the step between them. Where they are not
//! parallel (a bevel groove in a T- or corner joint) it is measured square to the face's chord
//! from the chord. The weld fills only that deep and the rest of the groove stays empty; the
//! second face then does not apply. Without `s` the whole groove is filled. An `s` deeper than
//! a closed groove fills the groove and returns a note; so does an `s` past the depth where a
//! groove that runs through opens on the root side (the thinner plate's thickness where the
//! plates' root sides are flush): full penetration. Neither is a refusal. [`GrooveWeld`]
//! returns the depth it filled.
//!
//! A concave face is built as asked and returns how deep it dips: below the chord, and below
//! each plate's outer face where the plates' normal is given (between plates in one plane that
//! is below the plate's surface: ISO 6520-1 no. 511). It is refused only when it would cross a
//! wall, with the deepest that builds ([`WeldFailure::ConcaveFaceCrossesWall`]).
//!
//! # Not covered
//!
//! Walls of lines and arcs only (conics and Béziers are refused). The section across one
//! station: a groove whose section changes along the joint is the caller's to find.

use std::f64::consts::TAU;

use crate::error::{Failure, Fit, FitMeasure, Reason, Result, SolidError};
use crate::math::{Point2, Vec2};
use crate::prism::profile::{check_regions, Loop2, ProfileRefusal, Region2, Seg2};

pub use crate::error::weld::{WeldFailure, WeldSide};

const OP: &str = "weld::groove";

/// The zone tolerance the section's arcs are checked at: the kernel's default
/// (`SM_ZONE_TOL_3D`), the tolerance its bodies are built at.
const ZONE_TOL: f64 = 1.0e-5;

/// Steps of the search for the deepest concave contour that builds (each halves the interval;
/// 60 take it below the rounding of any section size).
const FIT_STEPS: usize = 60;

/// How one side of the weld is closed between its toes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Contour {
    /// A straight chord between the toes: flush with plates in one plane.
    Flat,
    /// An arc through the toes, its crest `height` off the chord, the toes sharp.
    Convex {
        /// The crest's height above the chord (> 0).
        height: f64,
    },
    /// The convex arc with its toes blended: run out tangentially `overlap` past each toe.
    ConvexBlended {
        /// The crest's height above the chord (> 0).
        height: f64,
        /// How far the blend runs past each toe (>= 0; 0 is the plain convex face).
        overlap: f64,
    },
    /// An arc through the toes dipping `depth` below the chord.
    Concave {
        /// How deep the arc dips below the chord (> 0).
        depth: f64,
    },
}

/// One groove across its joint, as the weld tool reads it from the parts: the section of the
/// groove faces in the plane square to the joint.
#[derive(Debug, Clone, PartialEq)]
pub struct GrooveSection {
    /// The left part's groove face in the section plane, from its face-side toe away from the
    /// face: lines and arcs, each starting where the one before ends.
    pub left: Vec<Seg2>,
    /// The right part's groove face, the same way.
    pub right: Vec<Seg2>,
    /// The normal of the plates' outer faces on the face side, pointing out of the plates,
    /// where those faces are parallel (a butt joint, the plates in one plane or offset);
    /// `None` where they are not (a bevel groove in a T- or corner joint). It sets what the
    /// penetration depth is measured from.
    pub plate_normal: Option<Vec2>,
}

/// How the groove is filled: the user's options, every one set (ops gives the defaults).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FillOptions {
    /// The face's contour.
    pub face: Contour,
    /// The second face's contour for a weld from both sides (a symbol above and below the
    /// reference line); `None` for a weld from one side, whose root is flat. Used where the
    /// groove runs through the plate and is filled whole.
    pub second_face: Option<Contour>,
    /// The penetration depth `s` (see the module's docs for what it is measured from); `None`
    /// fills the whole groove.
    pub depth: Option<f64>,
}

/// What the weld tool tells the user about a weld it built.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum GrooveNote {
    /// The penetration depth asked for is deeper than the groove, which is closed where the
    /// parts touch: the whole groove is filled, no deeper.
    DeeperThanGroove {
        /// The depth asked for.
        asked: f64,
        /// The groove's depth.
        groove: f64,
    },
    /// The penetration depth asked for is past where the groove opens on the root side: the
    /// groove runs through and is filled whole (full penetration).
    DeeperThanPlate {
        /// The depth asked for.
        asked: f64,
        /// The depth where the groove opens on the root side (the thinner plate's thickness
        /// where the plates' root sides are flush).
        plate: f64,
    },
    /// A concave contour dips below the chord between its toes.
    BelowToes {
        /// Which side.
        side: WeldSide,
        /// How deep below the chord.
        depth: f64,
        /// How deep the face's lowest point lies below each plate's outer face on that side,
        /// left then right: square to the plates where their normal is given, else below the
        /// chord (both `depth`).
        below: [f64; 2],
        /// `depth` over the groove's depth (the plate's thickness where the groove runs
        /// through).
        ratio: f64,
    },
}

/// A groove weld across its joint.
#[derive(Debug, Clone, PartialEq)]
pub struct GrooveWeld {
    /// The weld's section: one counter-clockwise loop, no holes.
    pub region: Region2,
    /// How deep the groove reaches, measured as the penetration depth is.
    pub groove_depth: f64,
    /// How deep the weld fills it, measured the same way: the penetration depth where the
    /// weld ends inside the groove, else the groove's depth.
    pub filled_depth: f64,
    /// Whether the groove runs through the plate (the walls end apart).
    pub through: bool,
    /// What to tell the user.
    pub notes: Vec<GrooveNote>,
}

/// The weld that fills `section` with `options`: the groove between its walls filled, closed
/// by its faces.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for walls that are empty, not lines and arcs, not connected,
/// not finite, that do not run away from the face, that cross each other, a plates' normal
/// that is not finite or points into the plates, or a contour or depth that is not finite and
/// positive. A concave contour that would cross a wall is refused with the deepest that
/// builds ([`WeldFailure::ConcaveFaceCrossesWall`], its [`Fit`]).
pub fn fill(section: &GrooveSection, options: &FillOptions) -> Result<GrooveWeld> {
    check_input(section, options)?;
    match build(section, options)? {
        Ok(weld) => Ok(weld),
        Err(Crossing) => Err(concave_fit(section, options)?),
    }
}

/// The weld crosses itself.
struct Crossing;

/// The second face's contour: flat on a weld from one side.
fn second(options: &FillOptions) -> Contour {
    options.second_face.unwrap_or(Contour::Flat)
}

/// The weld, or [`Crossing`] when it crosses itself.
fn build(
    section: &GrooveSection,
    options: &FillOptions,
) -> Result<std::result::Result<GrooveWeld, Crossing>> {
    let (left, right) = (&section.left, &section.right);
    let (Some(l0), Some(r0), Some(lb), Some(rb)) = (
        left.first().map(Seg2::start),
        right.first().map(Seg2::start),
        left.last().map(Seg2::end),
        right.last().map(Seg2::end),
    ) else {
        return Err(SolidError::invalid_input(OP, "an empty wall"));
    };
    let eps = 1e-9 * scale(left.iter().chain(right));
    let chord = r0 - l0;
    let width = chord.length();
    if width <= eps {
        return Err(SolidError::invalid_input(OP, "the two toes at one point"));
    }
    let e = chord * (1.0 / width);
    let chord_up = left_normal(e);
    let under_chord = |p: Point2| -(p - l0).dot(chord_up);
    if under_chord(lb) <= eps || under_chord(rb) <= eps {
        return Err(SolidError::invalid_input(
            OP,
            "a wall that does not run away from the face (left and right swapped?)",
        ));
    }
    // what the penetration depth is measured from: the lower plate's outer face, square to
    // the plates, where they are parallel; else the chord
    let plates = match section.plate_normal {
        Some(n) => {
            let up = n * (1.0 / n.length());
            if up.dot(chord_up) <= 0.0 {
                return Err(SolidError::invalid_input(
                    OP,
                    "a plates' normal that points into the plates",
                ));
            }
            Some(up)
        }
        None => None,
    };
    let (origin, up) = match plates {
        Some(up) => (if (l0 - r0).dot(up) <= 0.0 { l0 } else { r0 }, up),
        None => (l0, chord_up),
    };
    let below = |p: Point2| -(p - origin).dot(up);
    let groove_depth = left
        .iter()
        .chain(right)
        .flat_map(|s| deepest_points(s, -up))
        .map(below)
        .fold(0.0_f64, f64::max);
    let through = lb.distance(rb) > eps;
    // where a groove that runs through opens on the root side: past it the weld is full
    // penetration
    let full = if through {
        below(lb).min(below(rb))
    } else {
        groove_depth
    };
    let mut notes = Vec::new();

    // the walls as far as the weld fills them, and the closure at their far ends
    let (mut left_wall, mut right_wall) = (left.clone(), right.clone());
    let mut bottom: Vec<Seg2> = Vec::new();
    let mut cut = false;
    let mut filled_depth = groove_depth;
    if let Some(s) = options.depth {
        if s < full - eps {
            let (Some(l), Some(r)) = (
                cut_chain(left, s, origin, up, eps),
                cut_chain(right, s, origin, up, eps),
            ) else {
                return Err(SolidError::invalid_input(
                    OP,
                    "a penetration depth that cuts one wall only",
                ));
            };
            let (Some(le), Some(re)) = (l.last().map(Seg2::end), r.last().map(Seg2::end)) else {
                return Err(SolidError::invalid_input(
                    OP,
                    "a penetration depth above a toe",
                ));
            };
            left_wall = l;
            right_wall = r;
            if re.distance(le) > eps {
                bottom.push(Seg2::Line { a: re, b: le });
            }
            cut = true;
            filled_depth = s;
        } else if s > full + eps {
            notes.push(if through {
                GrooveNote::DeeperThanPlate {
                    asked: s,
                    plate: full,
                }
            } else {
                GrooveNote::DeeperThanGroove {
                    asked: s,
                    groove: groove_depth,
                }
            });
        }
    }
    if through && !cut {
        bottom = closure(rb, lb, second(options));
    }

    // the loop, clockwise: face, right wall down, bottom, left wall up
    let face = closure(l0, r0, options.face);
    let mut segs = face.clone();
    segs.extend(right_wall);
    segs.extend(bottom.clone());
    segs.extend(left_wall.iter().rev().map(Seg2::reversed));
    if crosses(&segs, eps.max(1e-12) * 100.0) {
        return Ok(Err(Crossing));
    }
    let region = Region2 {
        outer: Loop2 { segs }.reversed(),
        holes: Vec::new(),
    };
    if let Err(refusal) = check_regions(std::iter::once(&region), eps.max(1e-12), ZONE_TOL) {
        return Err(SolidError::invalid_input(
            OP,
            match refusal {
                ProfileRefusal::LineOrArc => {
                    "a line or arc of the section that does not build (too short or too small)"
                }
                ProfileRefusal::Budget | ProfileRefusal::FreeForm => {
                    "a section that is not one closed loop"
                }
            },
        ));
    }
    if region.signed_area()?.sign() != Some(true) {
        return Err(SolidError::invalid_input(
            OP,
            "a section of no area (left and right swapped?)",
        ));
    }
    // a concave face's dip: below the chord, and below each plate's outer face on its side
    for (side, contour, closing, toes, out) in [
        (WeldSide::Face, options.face, &face, [l0, r0], plates),
        (
            WeldSide::Second,
            second(options),
            &bottom,
            [lb, rb],
            plates.map(|n| -n),
        ),
    ] {
        let used = side == WeldSide::Face || (through && !cut);
        let (true, Contour::Concave { depth }) = (used, contour) else {
            continue;
        };
        let below_toes = match out {
            Some(n) => {
                let lowest = |toe: Point2| {
                    closing
                        .iter()
                        .flat_map(|s| deepest_points(s, -n))
                        .map(|q| (toe - q).dot(n))
                        .fold(0.0_f64, f64::max)
                };
                [lowest(toes[0]), lowest(toes[1])]
            }
            None => [depth, depth],
        };
        notes.push(GrooveNote::BelowToes {
            side,
            depth,
            below: below_toes,
            ratio: depth / groove_depth,
        });
    }
    Ok(Ok(GrooveWeld {
        region,
        groove_depth,
        filled_depth,
        through,
        notes,
    }))
}

/// The refusal of a weld that crosses itself: when a concave contour is why, with the
/// deepest of it that builds.
fn concave_fit(section: &GrooveSection, options: &FillOptions) -> Result<SolidError> {
    let crossing = || {
        SolidError::invalid_input(
            OP,
            "a section that crosses itself (walls that cross, or a contour across a wall)",
        )
    };
    let builds = |o: &FillOptions| -> Result<bool> { Ok(build(section, o)?.is_ok()) };
    for side in [WeldSide::Face, WeldSide::Second] {
        let contour = match side {
            WeldSide::Face => options.face,
            WeldSide::Second => second(options),
        };
        let Contour::Concave { depth } = contour else {
            continue;
        };
        let with = |d: Contour| {
            let mut o = *options;
            match side {
                WeldSide::Face => o.face = d,
                WeldSide::Second => o.second_face = Some(d),
            }
            o
        };
        if !builds(&with(Contour::Flat))? {
            continue;
        }
        let (mut lo, mut hi) = (0.0, depth);
        for _ in 0..FIT_STEPS {
            let mid = 0.5 * (lo + hi);
            if builds(&with(Contour::Concave { depth: mid }))? {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        return Ok(Failure::new(
            OP,
            Reason::Weld(WeldFailure::ConcaveFaceCrossesWall { side, depth }),
        )
        .with_fit(Fit::new(lo, FitMeasure::Distance, None))
        .into_error());
    }
    Ok(crossing())
}

fn check_input(section: &GrooveSection, options: &FillOptions) -> Result<()> {
    let eps = 1e-9 * scale(section.left.iter().chain(&section.right));
    for wall in [&section.left, &section.right] {
        if wall.is_empty() {
            return Err(SolidError::invalid_input(OP, "an empty wall"));
        }
        for s in wall {
            if !s.is_line_or_arc() {
                return Err(SolidError::invalid_input(
                    OP,
                    "a wall segment that is not a line or an arc",
                ));
            }
            let ok = match *s {
                Seg2::Line { a, b } => finite(a) && finite(b),
                Seg2::Arc {
                    center,
                    radius,
                    start,
                    sweep,
                } => {
                    finite(center)
                        && radius.is_finite()
                        && radius > 0.0
                        && start.is_finite()
                        && sweep.is_finite()
                        && sweep.abs() < TAU
                }
                _ => false,
            };
            if !ok {
                return Err(SolidError::invalid_input(
                    OP,
                    "a wall segment that is not finite",
                ));
            }
        }
        if wall
            .iter()
            .zip(wall.iter().skip(1))
            .any(|(a, b)| a.end().distance(b.start()) > eps.max(1e-12) * 10.0)
        {
            return Err(SolidError::invalid_input(
                OP,
                "a wall whose segments do not join",
            ));
        }
    }
    let positive = |v: f64| v.is_finite() && v > 0.0;
    if section
        .plate_normal
        .is_some_and(|n| !(n.x.is_finite() && n.y.is_finite()) || n.length() == 0.0)
    {
        return Err(SolidError::invalid_input(
            OP,
            "a plates' normal that is not finite and non-zero",
        ));
    }
    for contour in [options.face, second(options)] {
        let ok = match contour {
            Contour::Flat => true,
            Contour::Convex { height } => positive(height),
            Contour::ConvexBlended { height, overlap } => {
                positive(height) && overlap.is_finite() && overlap >= 0.0
            }
            Contour::Concave { depth } => positive(depth),
        };
        if !ok {
            return Err(SolidError::invalid_input(
                OP,
                "a contour height or depth that is not finite and positive",
            ));
        }
    }
    if options.depth.is_some_and(|s| !positive(s)) {
        return Err(SolidError::invalid_input(
            OP,
            "a penetration depth that is not finite and positive",
        ));
    }
    Ok(())
}

fn finite(p: Point2) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

/// The size of the walls: their largest coordinate (arcs with their reach), at least 1.
fn scale<'a>(segs: impl Iterator<Item = &'a Seg2>) -> f64 {
    segs.fold(1.0_f64, |m, s| {
        let reach = match *s {
            Seg2::Arc { center, radius, .. } => center.x.abs().max(center.y.abs()) + radius,
            _ => 0.0,
        };
        m.max(s.start().max_dimension())
            .max(s.end().max_dimension())
            .max(reach)
    })
}

/// `v` turned a quarter turn counter-clockwise: the left of a run along `v`.
fn left_normal(v: Vec2) -> Vec2 {
    Vec2::new(-v.y, v.x)
}

/// The points of `s` that may lie furthest along `dir`: its ends, and an arc's point facing
/// `dir` where the arc passes it.
fn deepest_points(s: &Seg2, dir: Vec2) -> Vec<Point2> {
    let mut out = vec![s.start(), s.end()];
    if let Seg2::Arc { center, radius, .. } = *s {
        let p = center + dir * radius;
        if on_arc(s, p, 1e-12) {
            out.push(p);
        }
    }
    out
}

/// The path from `p` to `q` closing one side of the weld, its outside on the left of the run.
fn closure(p: Point2, q: Point2, contour: Contour) -> Vec<Seg2> {
    let chord = q - p;
    let c = chord.length();
    let e = chord * (1.0 / c);
    let n = left_normal(e);
    let m = (p + q) * 0.5;
    let a = 0.5 * c;
    match contour {
        Contour::Flat => vec![Seg2::Line { a: p, b: q }],
        Contour::Convex { height } => vec![sagitta_arc(p, q, height)],
        Contour::Concave { depth } => vec![sagitta_arc(p, q, -depth)],
        Contour::ConvexBlended {
            height: h,
            overlap: o,
        } => {
            if o <= 0.0 {
                return vec![sagitta_arc(p, q, h)];
            }
            let x = a + o;
            let sum = (x * x + h * h) / (2.0 * h);
            let big = a * sum / x;
            let small = sum - big;
            let at = |u: f64, v: f64| m + e * u + n * v;
            let c_main = at(0.0, h - big);
            let (c_left, c_right) = (at(-x, small), at(x, small));
            let inflect = |c_blend: Point2| c_main + (c_blend - c_main) * (big / sum);
            let (i_left, i_right) = (inflect(c_left), inflect(c_right));
            let (t_left, t_right) = (at(-x, 0.0), at(x, 0.0));
            vec![
                Seg2::Line { a: p, b: t_left },
                arc_about(c_left, t_left, i_left, true),
                arc_about(c_main, i_left, i_right, false),
                arc_about(c_right, i_right, t_right, true),
                Seg2::Line { a: t_right, b: q },
            ]
        }
    }
}

/// The arc from `p` to `q` whose middle stands `sagitta` off the chord, on the left of the run
/// when positive, on its right when negative.
fn sagitta_arc(p: Point2, q: Point2, sagitta: f64) -> Seg2 {
    let chord = q - p;
    let a = 0.5 * chord.length();
    let n = left_normal(chord * (1.0 / (2.0 * a)));
    let h = sagitta.abs();
    let r = (a * a + h * h) / (2.0 * h);
    let m = (p + q) * 0.5;
    // the centre lies off the middle on the chord's other side from the crest, or on its
    // side when the arc is more than half a circle (h > a)
    let center = m + n * (sagitta - sagitta.signum() * r);
    // bulging left of the run, the centre is on its right: the arc turns clockwise
    arc_about(center, p, q, sagitta < 0.0)
}

/// The arc about `center` from `p` to `q`, counter-clockwise when `ccw`.
fn arc_about(center: Point2, p: Point2, q: Point2, ccw: bool) -> Seg2 {
    let (dp, dq) = (p - center, q - center);
    let start = dp.y.atan2(dp.x);
    let end = dq.y.atan2(dq.x);
    let turn = (end - start).rem_euclid(TAU);
    let sweep = if ccw { turn } else { turn - TAU };
    Seg2::Arc {
        center,
        radius: dp.length(),
        start,
        sweep,
    }
}

/// `chain` up to where it first reaches `s` below the line through `origin` square to `up`,
/// ending there; `None` when it never does.
fn cut_chain(chain: &[Seg2], s: f64, origin: Point2, up: Vec2, eps: f64) -> Option<Vec<Seg2>> {
    let below = |p: Point2| -(p - origin).dot(up);
    let mut out = Vec::new();
    for seg in chain {
        let hit = match *seg {
            Seg2::Line { a, b } => {
                let (da, db) = (below(a) - s, below(b) - s);
                (da * db <= 0.0 && da != db).then(|| da / (da - db))
            }
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                // the circle meets the line where below(center + r u(t)) = s
                let d = s - below(center);
                // below(center + r u) = below(center) - r (u . up): u . up = -d / r
                let k = -d / radius;
                if k.abs() > 1.0 {
                    None
                } else {
                    let base = up.y.atan2(up.x);
                    let off = k.acos();
                    [base + off, base - off]
                        .into_iter()
                        .filter_map(|ang| {
                            let t = if sweep >= 0.0 {
                                (ang - start).rem_euclid(TAU)
                            } else {
                                (start - ang).rem_euclid(TAU)
                            };
                            (t <= sweep.abs() + eps / radius).then_some(t / sweep.abs())
                        })
                        .fold(None, |m: Option<f64>, t| Some(m.map_or(t, |m| m.min(t))))
                }
            }
            _ => None,
        };
        match hit {
            Some(t) if t <= eps => {
                return (!out.is_empty()).then_some(out);
            }
            Some(t) if t < 1.0 - eps => {
                out.push(part(seg, t));
                return Some(out);
            }
            Some(_) => {
                out.push(*seg);
                return Some(out);
            }
            None => out.push(*seg),
        }
    }
    None
}

/// The first fraction `t` of a line or arc.
fn part(seg: &Seg2, t: f64) -> Seg2 {
    match *seg {
        Seg2::Line { a, b } => Seg2::Line {
            a,
            b: a + (b - a) * t,
        },
        Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        } => Seg2::Arc {
            center,
            radius,
            start,
            sweep: sweep * t,
        },
        other => other,
    }
}

/// Whether the closed loop `segs` crosses or touches itself anywhere but where neighbours
/// join (`tol` round each join is not looked at).
fn crosses(segs: &[Seg2], tol: f64) -> bool {
    let n = segs.len();
    for (i, a) in segs.iter().enumerate() {
        for (j, b) in segs.iter().enumerate().skip(i + 1) {
            let next = j == i + 1;
            let wrap = i == 0 && j + 1 == n;
            let joins: Vec<Point2> = match (next, wrap) {
                (true, true) => vec![a.end(), a.start()],
                (true, false) => vec![a.end()],
                (false, true) => vec![a.start()],
                (false, false) => Vec::new(),
            };
            if meets(a, b, tol)
                .into_iter()
                .any(|p| joins.iter().all(|&v| p.distance(v) > tol))
            {
                return true;
            }
        }
    }
    false
}

/// Where two lines or arcs meet (where they overlap, the ends of the overlap).
fn meets(a: &Seg2, b: &Seg2, tol: f64) -> Vec<Point2> {
    let candidates = match (*a, *b) {
        (Seg2::Line { a: p, b: q }, Seg2::Line { a: r, b: s }) => line_line(p, q, r, s, tol),
        (Seg2::Line { a: p, b: q }, Seg2::Arc { center, radius, .. })
        | (Seg2::Arc { center, radius, .. }, Seg2::Line { a: p, b: q }) => {
            line_circle(p, q, center, radius)
        }
        (
            Seg2::Arc {
                center: c1,
                radius: r1,
                ..
            },
            Seg2::Arc {
                center: c2,
                radius: r2,
                ..
            },
        ) => {
            if c1.distance(c2) <= tol && (r1 - r2).abs() <= tol {
                // one circle: the arcs meet where an end of one lies on the other
                vec![a.start(), a.end(), b.start(), b.end()]
            } else {
                circle_circle(c1, r1, c2, r2)
            }
        }
        _ => Vec::new(),
    };
    candidates
        .into_iter()
        .filter(|&p| on_seg(a, p, tol) && on_seg(b, p, tol))
        .collect()
}

/// Whether `p` lies on the line or arc `s`, within `tol`.
fn on_seg(s: &Seg2, p: Point2, tol: f64) -> bool {
    match *s {
        Seg2::Line { a, b } => {
            let d = b - a;
            let len2 = d.length_squared();
            if len2 == 0.0 {
                return p.distance(a) <= tol;
            }
            let t = (p - a).dot(d) / len2;
            let near = a + d * t.clamp(0.0, 1.0);
            p.distance(near) <= tol
        }
        Seg2::Arc { center, radius, .. } => {
            (p.distance(center) - radius).abs() <= tol && on_arc(s, p, tol)
        }
        _ => false,
    }
}

/// Whether the direction of `p` from an arc's centre lies within its sweep (`tol` along it).
fn on_arc(s: &Seg2, p: Point2, tol: f64) -> bool {
    let Seg2::Arc {
        center,
        radius,
        start,
        sweep,
    } = *s
    else {
        return false;
    };
    let d = p - center;
    let ang = d.y.atan2(d.x);
    let t = if sweep >= 0.0 {
        (ang - start).rem_euclid(TAU)
    } else {
        (start - ang).rem_euclid(TAU)
    };
    let slack = tol / radius;
    t <= sweep.abs() + slack || t >= TAU - slack
}

fn line_line(p: Point2, q: Point2, r: Point2, s: Point2, tol: f64) -> Vec<Point2> {
    let (d1, d2) = (q - p, s - r);
    let den = d1.x * d2.y - d1.y * d2.x;
    let scale = d1.length() * d2.length();
    if den.abs() <= 1e-14 * scale {
        // parallel: they meet only if they lie on one line, at the ends of the overlap
        let off = (r - p).x * d1.y - (r - p).y * d1.x;
        if off.abs() <= tol * d1.length() {
            return vec![p, q, r, s];
        }
        return Vec::new();
    }
    let w = r - p;
    let t = (w.x * d2.y - w.y * d2.x) / den;
    vec![p + d1 * t]
}

fn line_circle(p: Point2, q: Point2, c: Point2, r: f64) -> Vec<Point2> {
    let d = q - p;
    let f = p - c;
    let a = d.dot(d);
    if a == 0.0 {
        return Vec::new();
    }
    let b = 2.0 * f.dot(d);
    let cc = f.dot(f) - r * r;
    let disc = b * b - 4.0 * a * cc;
    if disc < 0.0 {
        // a near miss within rounding counts as touching at the foot of the centre
        let t = -b / (2.0 * a);
        return vec![p + d * t];
    }
    let root = disc.sqrt();
    vec![
        p + d * ((-b - root) / (2.0 * a)),
        p + d * ((-b + root) / (2.0 * a)),
    ]
}

fn circle_circle(c1: Point2, r1: f64, c2: Point2, r2: f64) -> Vec<Point2> {
    let d = c2 - c1;
    let dist = d.length();
    if dist == 0.0 {
        return Vec::new();
    }
    let a = (dist * dist + r1 * r1 - r2 * r2) / (2.0 * dist);
    let h2 = r1 * r1 - a * a;
    let u = d * (1.0 / dist);
    let mid = c1 + u * a;
    if h2 < 0.0 {
        // a near miss within rounding counts as touching on the line of centres
        return vec![mid];
    }
    let h = h2.sqrt();
    let n = left_normal(u);
    vec![mid + n * h, mid - n * h]
}
