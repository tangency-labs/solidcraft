//! Butt and groove welds: the section of one weld across a joint, filling the groove that is
//! already modelled in the parts.
//!
//! Not upstream: SolidCraft's own (#15132, #18500). The section is plain planar geometry
//! ([`Region2`]): the caller cuts the groove faces of both parts with the plane square to the
//! joint and passes the two walls in; nothing here sees or changes a part. How the section
//! becomes a body along the joint is a later step.
//!
//! | Rust               | What it is                                                         |
//! |--------------------|--------------------------------------------------------------------|
//! | [`GrooveWeld`]     | The two walls, the contour of each face, the penetration depth     |
//! | [`Contour`]        | Flat, convex (sharp or blended toes) or concave                    |
//! | [`groove_section`] | The weld's section and its notices                                 |
//! | [`GrooveSection`]  | The region, the groove's depth, whether it runs through the plate  |
//! | [`GrooveNotice`]   | What the weld tool tells the user (not refusals)                   |
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
//! The *face* closes the section between the two toes. Where the groove runs through the plate
//! the other side is closed between the two wall ends: flat on a weld from one side (its root,
//! always flush), or by the *second face* of a weld from both sides (a weld symbol above and
//! below the reference line: X, K, double-U, double-J, or any groove the user welds from both
//! sides), which follows the user's contour for that side. Each face is [`Contour::Flat`] (a
//! straight chord, the default), [`Contour::Convex`] (an arc through the toes with its crest
//! the given height off the chord), [`Contour::ConvexBlended`] (the same crest, run out
//! tangentially past the toes) or [`Contour::Concave`] (an arc dipping the given depth below
//! the chord). The two faces of a weld from both sides are set apart, each flat by default.
//!
//! **Blended toes** (ISO 2553, "toes shall be blended smoothly"): a convex arc of radius `R`
//! between the toes and at each toe a concave arc of radius `r`, tangent to the chord's line
//! at its outer end and to the convex arc at the toe. The user gives the crest height `h`; the
//! blend runs `o` past each toe (by default `o = h`). With half the chord `a` and
//! `x = a + o`: `R + r = (x² + h²) / 2h`, `R = a (R + r) / x`. `o = 0` is the plain convex
//! face. The blend runs along the chord's line, so it assumes the part's outer faces lie on
//! it (plates in one plane, toes at one height).
//!
//! **Penetration depth `s`**, measured from the face's chord: the weld fills only that deep
//! and the rest of the groove stays empty; the second face then does not apply. Without `s`
//! the whole groove is filled. An `s` deeper than the groove fills the groove and returns a
//! notice, as does an `s` past the plate: neither is a refusal.
//!
//! A concave face is built as asked and returns how deep it dips below the chord (between
//! plates in one plane that is below the plate's surface: ISO 6520-1 no. 511). It is refused
//! only when it would cross a wall, with the deepest that builds.
//!
//! # Not covered
//!
//! Walls of lines and arcs only (conics and Béziers are refused). The section across one
//! station: a groove whose section changes along the joint is the caller's to find.

use std::f64::consts::TAU;

use crate::error::{Failure, Fit, FitMeasure, Reason, Result, SolidError};
use crate::math::{Point2, Vec2};
use crate::prism::profile::{check_regions, Loop2, ProfileRefusal, Region2, Seg2};

const OP: &str = "weld::groove";

/// The zone tolerance the section's arcs are checked at: the kernel's default
/// (`SM_ZONE_TOL_3D`), the tolerance its bodies are built at.
const ZONE_TOL: f64 = 1.0e-5;

/// Steps of the search for the deepest concave contour that builds (each halves the interval;
/// 60 take it below the rounding of any section size).
const FIT_STEPS: usize = 60;

/// How one side of the weld is closed between its toes.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Contour {
    /// A straight chord between the toes: flush with plates in one plane. The default.
    #[default]
    Flat,
    /// An arc through the toes, its crest `height` off the chord, the toes sharp.
    Convex {
        /// The crest's height above the chord (> 0).
        height: f64,
    },
    /// The convex arc with its toes blended: run out tangentially `overlap` past each toe
    /// (`None`: `overlap = height`).
    ConvexBlended {
        /// The crest's height above the chord (> 0).
        height: f64,
        /// How far the blend runs past each toe (>= 0); `None` for the crest height.
        overlap: Option<f64>,
    },
    /// An arc through the toes dipping `depth` below the chord.
    Concave {
        /// How deep the arc dips below the chord (> 0).
        depth: f64,
    },
}

/// One groove weld's section, as the weld tool reads it from the parts.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GrooveWeld {
    /// The left part's groove face in the section plane, from its face-side toe away from the
    /// face: lines and arcs, each starting where the one before ends.
    pub left: Vec<Seg2>,
    /// The right part's groove face, the same way.
    pub right: Vec<Seg2>,
    /// The face's contour.
    pub face: Contour,
    /// The second face's contour for a weld from both sides (a symbol above and below the
    /// reference line); `None` for a weld from one side, whose root is flat. Used where the
    /// groove runs through the plate and is filled whole.
    pub second_face: Option<Contour>,
    /// The penetration depth `s` from the face's chord; `None` fills the whole groove.
    pub depth: Option<f64>,
}

/// Which side of the weld a notice is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WeldSide {
    /// The face (the side the walls start from).
    Face,
    /// The second face of a weld from both sides.
    Second,
}

/// What the weld tool tells the user about a section it built.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GrooveNotice {
    /// The penetration depth asked for is deeper than the groove, which is closed where the
    /// parts touch: the whole groove is filled, no deeper.
    DeeperThanGroove {
        /// The depth asked for.
        asked: f64,
        /// The groove's depth.
        groove: f64,
    },
    /// The penetration depth asked for is past the plate: the groove runs through and is
    /// filled whole (full penetration).
    DeeperThanPlate {
        /// The depth asked for.
        asked: f64,
        /// The plate's thickness at the joint (the groove's depth).
        plate: f64,
    },
    /// A concave contour dips below the chord between its toes.
    BelowToes {
        /// Which side.
        side: WeldSide,
        /// How deep below the chord.
        depth: f64,
        /// `depth` over the groove's depth (the plate's thickness where the groove runs
        /// through).
        ratio: f64,
    },
}

/// A groove weld's section.
#[derive(Debug, Clone, PartialEq)]
pub struct GrooveSection {
    /// The weld's section: one counter-clockwise loop, no holes.
    pub region: Region2,
    /// How deep the groove reaches below the face's chord.
    pub groove_depth: f64,
    /// Whether the groove runs through the plate (the walls end apart).
    pub through: bool,
    /// What to tell the user.
    pub notices: Vec<GrooveNotice>,
}

/// The section of `weld`: the groove between its walls filled, closed by its faces.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for walls that are empty, not lines and arcs, not connected,
/// not finite, that do not run away from the face, that cross each other, or a contour or
/// depth that is not finite and positive. A concave contour that would cross a wall is refused
/// with the deepest that builds ([`SolidError::fit`]).
pub fn groove_section(weld: &GrooveWeld) -> Result<GrooveSection> {
    check_input(weld)?;
    match build(weld)? {
        Ok(section) => Ok(section),
        Err(Crossing) => Err(concave_fit(weld)?),
    }
}

/// The section crosses itself.
struct Crossing;

/// The section, or [`Crossing`] when it crosses itself.
fn build(weld: &GrooveWeld) -> Result<std::result::Result<GrooveSection, Crossing>> {
    let (left, right) = (&weld.left, &weld.right);
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
    let up = left_normal(e);
    let below = |p: Point2| -(p - l0).dot(up);
    if below(lb) <= eps || below(rb) <= eps {
        return Err(SolidError::invalid_input(
            OP,
            "a wall that does not run away from the face (left and right swapped?)",
        ));
    }
    let groove_depth = left
        .iter()
        .chain(right)
        .flat_map(|s| deepest_points(s, -up))
        .map(below)
        .fold(0.0_f64, f64::max);
    let through = lb.distance(rb) > eps;
    let mut notices = Vec::new();

    // the walls as far as the weld fills them, and the closure at their far ends
    let (mut left_wall, mut right_wall) = (left.clone(), right.clone());
    let mut bottom: Vec<Seg2> = Vec::new();
    let mut cut = false;
    if let Some(s) = weld.depth {
        if s < groove_depth - eps {
            let (Some(l), Some(r)) = (
                cut_chain(left, s, l0, up, eps),
                cut_chain(right, s, l0, up, eps),
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
        } else if s > groove_depth + eps {
            notices.push(if through {
                GrooveNotice::DeeperThanPlate {
                    asked: s,
                    plate: groove_depth,
                }
            } else {
                GrooveNotice::DeeperThanGroove {
                    asked: s,
                    groove: groove_depth,
                }
            });
        }
    }
    if through && !cut {
        bottom = closure(rb, lb, weld.second_face.unwrap_or_default());
    }

    // the loop, clockwise: face, right wall down, bottom, left wall up
    let mut segs = closure(l0, r0, weld.face);
    segs.extend(right_wall);
    segs.extend(bottom);
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
    for (side, contour, used) in [
        (WeldSide::Face, weld.face, true),
        (
            WeldSide::Second,
            weld.second_face.unwrap_or_default(),
            through && !cut,
        ),
    ] {
        if let (true, Contour::Concave { depth }) = (used, contour) {
            notices.push(GrooveNotice::BelowToes {
                side,
                depth,
                ratio: depth / groove_depth,
            });
        }
    }
    Ok(Ok(GrooveSection {
        region,
        groove_depth,
        through,
        notices,
    }))
}

/// The refusal of a section that crosses itself: when a concave contour is why, with the
/// deepest of it that builds.
fn concave_fit(weld: &GrooveWeld) -> Result<SolidError> {
    let crossing = || {
        SolidError::invalid_input(
            OP,
            "a section that crosses itself (walls that cross, or a contour across a wall)",
        )
    };
    let builds = |w: &GrooveWeld| -> Result<bool> { Ok(build(w)?.is_ok()) };
    for side in [WeldSide::Face, WeldSide::Second] {
        let contour = match side {
            WeldSide::Face => weld.face,
            WeldSide::Second => weld.second_face.unwrap_or_default(),
        };
        let Contour::Concave { depth } = contour else {
            continue;
        };
        let with = |d: Contour| {
            let mut w = weld.clone();
            match side {
                WeldSide::Face => w.face = d,
                WeldSide::Second => w.second_face = Some(d),
            }
            w
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
        let what = match side {
            WeldSide::Face => "a concave face deeper than the groove's walls allow",
            WeldSide::Second => "a concave second face deeper than the groove's walls allow",
        };
        return Ok(Failure::new(OP, Reason::Failed(what))
            .with_fit(Fit::new(lo, FitMeasure::Distance, None))
            .into_error());
    }
    Ok(crossing())
}

/// The input checked: walls of finite lines and arcs, each connected; contours and depth
/// finite and positive.
fn check_input(weld: &GrooveWeld) -> Result<()> {
    let eps = 1e-9 * scale(weld.left.iter().chain(&weld.right));
    for wall in [&weld.left, &weld.right] {
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
    for contour in [weld.face, weld.second_face.unwrap_or_default()] {
        let ok = match contour {
            Contour::Flat => true,
            Contour::Convex { height } => positive(height),
            Contour::ConvexBlended { height, overlap } => {
                positive(height) && overlap.is_none_or(|o| o.is_finite() && o >= 0.0)
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
    if weld.depth.is_some_and(|s| !positive(s)) {
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
        Contour::ConvexBlended { height: h, overlap } => {
            let o = overlap.unwrap_or(h);
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
