//! Filling a channel: the region between two walls, closed across their open ends by a face
//! of a chosen contour, as a planar section.
//!
//! Not upstream: SolidCraft's own (#15132, #18500). Generic geometry for any client: a
//! [`Channel`] (two walls and, where given, the normal the depth is measured along) and
//! [`FillOptions`] (each face's [`Contour`] and a depth) give [`fill`], which returns the
//! filled section ([`Filled`]) or a typed refusal with the nearest size that builds
//! ([`FillFailure`]). It reads and changes no body. What the region stands for, and how a
//! refusal is worded to a user, is the caller's.
//!
//! | Rust              | What it is                                                           |
//! |-------------------|----------------------------------------------------------------------|
//! | [`Channel`]       | The channel: its two walls and the depth's normal                    |
//! | [`FillOptions`]   | The contour of each face and the depth `s`                           |
//! | [`Contour`]       | Flat, convex (sharp or blended toes) or concave                      |
//! | [`fill`]          | The channel filled, closed by its faces                              |
//! | [`Filled`]        | The filled section, the channel's and the filled depth, its notes    |
//! | [`FillNote`]      | What a caller may want to report about a fill that built             |
//! | [`FillFailure`]   | Why a fill is refused (`error::fill`)                                |
//!
//! The parts it is made of are operations of their own: [`arc_clearance`] (the deepest arc
//! through two points that clears a set of sides, in closed form), [`close`] (a face from its
//! contour: a chord, a sagitta arc, or one with blended toes), [`cut_chain`] (a chain of
//! lines and arcs up to a depth along a direction) and
//! [`crate::prism::profile::loop_crosses`] (the loop self-crossing check, which the fill and
//! a retry at a fit both ask).
//!
//! The options have no defaults here: the caller sets every one.
//!
//! # The geometry
//!
//! The fill fills whatever channel the walls bound, whatever its shape: walls of lines and
//! arcs, sloped, stepped or rounded.
//!
//! Each wall is a chain of lines and arcs. It runs from its *toe* on the face side away from
//! the face. Seen from the face side, `left` is on the left. A wall ends where the channel
//! ends: apart from the other wall when the channel is open on the far side, or where the two
//! walls meet (both walls then end at the same point, the channel is closed).
//!
//! The *face* closes the region between the two toes. Where the channel is open on the far
//! side, that side is closed between the two wall ends: flat (a chord), or by a *second face*
//! of its own contour. Each face is [`Contour::Flat`] (a straight chord), [`Contour::Convex`]
//! (an arc through the toes with its crest the given height off the chord),
//! [`Contour::ConvexBlended`] (the same crest, run out tangentially past the toes) or
//! [`Contour::Concave`] (an arc dipping the given depth below the chord).
//!
//! **Blended toes**: a convex arc of radius `R` between the toes and at each toe a concave arc
//! of radius `r`, tangent to the chord's line at its outer end and to the convex arc at the
//! toe. The crest height is `h`, the blend runs `o` past each toe. With half the chord `a` and
//! `x = a + o`: `R + r = (x² + h²) / 2h`, `R = a (R + r) / x`. `o = 0` is the plain convex
//! face. Every arc is a polyline arc, the chord between its ends and its bulge
//! (`prism::arc_conversion::bulge_arc`); none is more than half a circle, so a convex face higher
//! than half the distance between its toes is refused. The blend runs along the chord's line,
//! so it is built only where the sides beyond the toes on that side do: [`Channel::depth_normal`]
//! given and square to the chord. Elsewhere it is refused ([`FillFailure::BlendedToesOffChord`]).
//! An overlap no longer than the zone tolerance is built with sharp toes
//! ([`FillNote::SharpToes`]), within that tolerance of the blended face: a height the blended
//! face allows (up to half the chord plus the overlap) is built, its crest at most half the
//! chord.
//!
//! **Depth `s`.** Measured from the deeper toe along `-depth_normal` where
//! [`Channel::depth_normal`] is given, and the fill's bottom runs square to it; else from the
//! chord, square to it. The fill reaches only that deep and the rest of the channel stays
//! empty; the second face then does not apply. Without `s` the whole channel is filled. An `s`
//! deeper than a closed channel fills it whole and returns a note
//! ([`FillNote::DeeperThanChannel`]); so does an `s` past the depth where an open channel opens
//! on the far side, the shallower wall end ([`FillNote::DeeperThanOpening`]). Neither is a
//! refusal. [`Filled`] returns the depth it filled.
//!
//! A concave face is built as asked and returns how deep it dips: below the chord, and below
//! each toe's level along `depth_normal` where it is given ([`FillNote::BelowToes`]). Its
//! limit is closed form: the depth `F` where it first touches another side of the section (a
//! wall, the bottom, the other face); the deepest that builds is `F - δ`, δ the depth margin
//! (the larger of the section's conservative allowance and the kernel's zone tolerance),
//! refused past it ([`FillFailure::ConcaveFaceCrossesWall`]). With nothing touched, the half
//! circle builds and deeper is refused ([`FillFailure::ConcaveFaceTooDeep`]). A face no higher
//! or deeper than the zone tolerance is built flat ([`FillNote::BuiltFlat`]); one past that but
//! shallower than the smallest arc the profile builds is refused
//! ([`FillFailure::FaceTooShallow`]).
//!
//! The crossing check and [`arc_clearance`] decide at the near-miss tolerance
//! ([`near_miss`]), not at a proved bound: `fill` is a construction that keeps those margins.
//!
//! # Not covered
//!
//! Walls of lines and arcs only (conics and Béziers are refused). One planar section: a
//! channel whose section changes along its length is the caller's to slice.

use crate::math::det;
use std::f64::consts::{PI, TAU};

use crate::error::{Failure, Fit, FitMeasure, Reason, Result, SolidError};
use crate::kernel::Kernel;
use crate::math::{Point2, Vec2};
use crate::prism::arc_conversion::{bulge_arc, Vertex};
use crate::prism::profile::{
    check_regions, gamma, loop_crosses, near_miss, on_arc, on_seg, smallest_arc_sweep, Loop2,
    ProfileRefusal, Region2, Seg2, MAX_ARC_RADIUS, MAX_SEGMENTS,
};

pub use crate::error::fill::{FillFailure, FillSide};

const OP: &str = "prism::fill";

/// How one side of the fill is closed between its toes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Contour {
    /// A straight chord between the toes.
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

/// One channel: its two walls in the section plane.
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    /// The left wall, from its face-side toe away from the face: lines and arcs, each
    /// starting where the one before ends.
    pub left: Vec<Seg2>,
    /// The right wall, the same way.
    pub right: Vec<Seg2>,
    /// The direction the depth is measured against, pointing away from the channel (out of
    /// the face side): the depth runs from the deeper toe along its opposite, and the sides
    /// beyond the toes run square to it. `None`: the depth runs from the chord, square to it,
    /// and blended toes are refused.
    pub depth_normal: Option<Vec2>,
}

/// How the channel is filled: every option set (the caller gives the defaults).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FillOptions {
    /// The face's contour.
    pub face: Contour,
    /// The second face's contour, closing the far side between the wall ends; `None` for a
    /// flat one. Used where the channel is open on the far side and filled whole.
    pub second_face: Option<Contour>,
    /// The depth `s` (see the module's docs for what it is measured from); `None` fills the
    /// whole channel.
    pub depth: Option<f64>,
}

/// What a caller may want to report about a fill that built.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum FillNote {
    /// The depth asked for is deeper than the channel, which is closed where its walls meet:
    /// the whole channel is filled, no deeper.
    DeeperThanChannel {
        /// The depth asked for.
        asked: f64,
        /// The channel's depth.
        channel: f64,
    },
    /// The depth asked for is past where the channel opens on the far side: it is filled
    /// whole.
    DeeperThanOpening {
        /// The depth asked for.
        asked: f64,
        /// The depth where the channel opens on the far side (the shallower wall end's depth).
        opening: f64,
    },
    /// A face asked for no higher or deeper than the kernel's zone tolerance: built flat (no
    /// arc builds that shallow).
    BuiltFlat {
        /// Which side.
        side: FillSide,
        /// The height or depth asked for.
        asked: f64,
    },
    /// Blended toes asked with an overlap no longer than the kernel's zone tolerance: built with
    /// sharp toes (the blend's end is one point with the toe), the crest at most half the chord.
    SharpToes {
        /// Which side.
        side: FillSide,
        /// The overlap asked for.
        overlap: f64,
        /// The height asked for.
        asked: f64,
        /// The sharp face's height: the one asked, at most half the chord where the blended face
        /// allows more (a height at or below the zone tolerance is then built flat, with
        /// [`FillNote::BuiltFlat`] too).
        height: f64,
    },
    /// A concave contour dips below the chord between its toes.
    BelowToes {
        /// Which side.
        side: FillSide,
        /// How deep below the chord.
        depth: f64,
        /// How deep the face's lowest point lies below each toe's level, left then right:
        /// along `depth_normal` where it is given, else below the chord (both `depth`).
        below: [f64; 2],
        /// `depth` over the channel's depth.
        ratio: f64,
    },
}

/// A filled channel.
#[derive(Debug, Clone, PartialEq)]
pub struct Filled {
    /// The filled section: one counter-clockwise loop, no holes.
    pub region: Region2,
    /// How deep the channel reaches, measured as the depth `s` is.
    pub channel_depth: f64,
    /// How deep it is filled, measured the same way: the depth `s` where the fill ends
    /// inside the channel, else the channel's depth.
    pub filled_depth: f64,
    /// Whether the channel is open on the far side (the walls end apart).
    pub through: bool,
    /// What a caller may report.
    pub notes: Vec<FillNote>,
}

/// The fill of `section` with `options`: the channel between its walls filled, closed
/// by its faces.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for walls that are empty, not lines and arcs, not connected,
/// not finite, that do not run away from the face, that cross each other, a depth normal
/// that is not finite or points into the channel, or a contour or depth that is not finite and
/// positive. A face this construction does not build is refused as a [`FillFailure`]: a
/// convex face past half a circle ([`FillFailure::ConvexFaceTooHigh`]), a concave face that
/// reaches the section's other sides ([`FillFailure::ConcaveFaceCrossesWall`]) or dips past
/// half a circle ([`FillFailure::ConcaveFaceTooDeep`]), each with its limit as its [`Fit`], in
/// closed form; blended toes where the sides beyond the toes do not run along the chord
/// ([`FillFailure::BlendedToesOffChord`]).
pub fn fill(k: &Kernel, section: &Channel, options: &FillOptions) -> Result<Filled> {
    check_input(section, options)?;
    build(k, section, options)
}

/// The depth margin a face of `section`'s fill keeps from what it touches (`delta` in [`fill`]):
/// the larger of the section's conservative allowance (1e-9 of its size, [`scale`]) and the
/// kernel's zone tolerance. The fill reads it here, and so does a caller that compares walls
/// against it (one implementation; brett19's point 5 on #22504).
#[must_use]
pub fn margin(k: &Kernel, section: &Channel) -> f64 {
    margin_of(
        1e-9 * scale(section.left.iter().chain(&section.right)),
        k.tolerances.zone_tol_3d,
    )
}

/// [`margin`] from the section's allowance `eps` and the zone tolerance `zone`, for a caller
/// that has `eps` already ([`build`]).
fn margin_of(eps: f64, zone: f64) -> f64 {
    eps.max(zone)
}

/// The second face's contour: flat without one.
fn second(options: &FillOptions) -> Contour {
    options.second_face.unwrap_or(Contour::Flat)
}

/// The fill.
fn build(k: &Kernel, section: &Channel, options: &FillOptions) -> Result<Filled> {
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
    // what the depth is measured from: the deeper toe, along the depth normal where given;
    // else the chord
    let normal = match section.depth_normal {
        Some(n) => {
            // scaled by its larger part first, so neither its length's overflow nor its
            // underflow matters
            let m = n.x.abs().max(n.y.abs());
            let n = Vec2::new(n.x / m, n.y / m);
            let up = n * (1.0 / n.length());
            if up.dot(chord_up) <= 0.0 {
                return Err(SolidError::invalid_input(
                    OP,
                    "a depth normal that points into the channel",
                ));
            }
            Some(up)
        }
        None => None,
    };
    let (origin, up) = match normal {
        Some(up) => (if (l0 - r0).dot(up) <= 0.0 { l0 } else { r0 }, up),
        None => (l0, chord_up),
    };
    let below = |p: Point2| -(p - origin).dot(up);
    let channel_depth = left
        .iter()
        .chain(right)
        .flat_map(|s| deepest_points(s, -up))
        .map(below)
        .fold(0.0_f64, f64::max);
    if channel_depth <= eps {
        return Err(SolidError::invalid_input(
            OP,
            "a channel of no depth below the deeper toe",
        ));
    }
    let through = lb.distance(rb) > eps;
    // where an open channel opens on the far side: past it the channel is filled whole
    let full = if through {
        below(lb).min(below(rb))
    } else {
        channel_depth
    };
    let mut notes = Vec::new();

    // the walls as far as the fill reaches, and the closure at their far ends
    let (mut left_wall, mut right_wall) = (left.clone(), right.clone());
    let mut bottom: Vec<Seg2> = Vec::new();
    let mut cut = false;
    let mut filled_depth = channel_depth;
    if let Some(s) = options.depth {
        if s < full - eps {
            let (Some(l), Some(r)) = (
                cut_chain(left, s, origin, up, eps),
                cut_chain(right, s, origin, up, eps),
            ) else {
                return Err(SolidError::invalid_input(
                    OP,
                    "a depth that cuts one wall only",
                ));
            };
            let (Some(le), Some(re)) = (l.last().map(Seg2::end), r.last().map(Seg2::end)) else {
                return Err(SolidError::invalid_input(OP, "a depth above a toe"));
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
                FillNote::DeeperThanOpening {
                    asked: s,
                    opening: full,
                }
            } else {
                FillNote::DeeperThanChannel {
                    asked: s,
                    channel: channel_depth,
                }
            });
        }
    }
    // the two faces: what each would be, its own limits first (blended toes before the
    // heights: a height fit cannot make blended toes build where the sides do not allow them)
    let second_used = through && !cut;
    let zone = k.tolerances.zone_tol_3d;
    let mut shallow_options = *options;
    // the sides beyond a face's toes run along its chord: the depth normal given, square to it
    let along = |p: Point2, q: Point2| normal.is_some_and(|n| (q - p).dot(n).abs() <= eps);
    shallow_options.face = toes(
        FillSide::Face,
        options.face,
        0.5 * l0.distance(r0),
        along(l0, r0),
        (zone, eps),
        &mut notes,
    )?;
    if second_used {
        shallow_options.second_face = Some(toes(
            FillSide::Second,
            second(options),
            0.5 * rb.distance(lb),
            along(rb, lb),
            (zone, eps),
            &mut notes,
        )?);
    }
    // the profile's budget, before any fit is offered and before the pairwise check: the loop's
    // lines and arcs as they will be built (long walls cut short count only as cut). Blended
    // toes that stay blended close in five pieces (a line, three arcs, a line); every other face
    // in one (an arc, or the chord, also where it is built flat)
    let pieces = |contour: Contour| match contour {
        Contour::ConvexBlended { height, overlap } if overlap > 0.0 && height > zone => 5,
        _ => 1,
    };
    let count = pieces(shallow_options.face)
        + right_wall.len()
        + left_wall.len()
        + if second_used {
            pieces(second(&shallow_options))
        } else {
            bottom.len()
        };
    if count > MAX_SEGMENTS {
        return Err(SolidError::invalid_input(
            OP,
            "a section of more lines and arcs than a profile takes (MAX_SEGMENTS)",
        ));
    }
    let face_floor;
    (shallow_options.face, face_floor) = shallow(
        FillSide::Face,
        l0,
        r0,
        shallow_options.face,
        zone,
        &mut notes,
    )?;
    let mut second_floor = 0.0;
    if second_used {
        let (contour, floor) = shallow(
            FillSide::Second,
            rb,
            lb,
            second(&shallow_options),
            zone,
            &mut notes,
        )?;
        shallow_options.second_face = Some(contour);
        second_floor = floor;
    }
    let options = &shallow_options;
    if second_used {
        check_face(FillSide::Second, rb, lb, second(options), eps)?;
    }
    check_face(FillSide::Face, l0, r0, options.face, eps)?;
    let second_path = if second_used {
        close(rb, lb, second(options))
    } else {
        None
    };
    let face_path = close(l0, r0, options.face);
    // a concave face: the deepest that touches none of the section's other sides
    let walls: Vec<Seg2> = right_wall.iter().chain(&left_wall).copied().collect();
    let mut limits = Vec::new();
    if let Contour::Concave { depth } = options.face {
        let mut others = walls.clone();
        if second_used {
            others.extend(
                second_path
                    .clone()
                    .unwrap_or_else(|| vec![Seg2::Line { a: rb, b: lb }]),
            );
        } else {
            others.extend(bottom.iter().copied());
        }
        let (fit, contact) = arc_clearance(l0, r0, &others, eps);
        limits.push((FillSide::Face, depth, fit, contact, face_floor));
    }
    if let (true, Contour::Concave { depth }) = (second_used, second(options)) {
        let mut others = walls;
        others.extend(
            face_path
                .clone()
                .unwrap_or_else(|| vec![Seg2::Line { a: l0, b: r0 }]),
        );
        let (fit, contact) = arc_clearance(rb, lb, &others, eps);
        limits.push((FillSide::Second, depth, fit, contact, second_floor));
    }
    // delta, the depth margin a face keeps from what it touches (each side grown by twice the
    // crossing check's near-miss tolerance, in arc_clearance): the larger of the section's
    // conservative allowance (eps, 1e-9 of its size) and the kernel's zone tolerance. A concave
    // face builds from its arc floor up to delta short of the touch (with nothing touched, up
    // to the half circle, which builds). Outside that, its fit is the nearest size that
    // builds: the floor below it, the deepest past it, the zone tolerance (built flat) where
    // the deepest is below the floor. Every fit builds when retried.
    let delta = margin_of(eps, zone);
    for &(side, depth, touch, contact, floor) in &limits {
        let (upper, slack) = if contact {
            (touch - delta, 0.0)
        } else {
            (touch, eps)
        };
        if depth >= floor && depth <= upper + slack {
            continue;
        }
        let why = if depth < floor {
            FillFailure::FaceTooShallow { side, size: depth }
        } else if contact {
            FillFailure::ConcaveFaceCrossesWall { side, depth }
        } else {
            FillFailure::ConcaveFaceTooDeep { side, depth }
        };
        let fit = if upper < floor {
            zone
        } else if depth < floor {
            floor
        } else {
            upper
        };
        return Err(Failure::new(OP, Reason::Fill(why))
            .with_fit(Fit::new(fit, FitMeasure::Distance, None))
            .into_error());
    }
    if second_used {
        bottom = second_path.ok_or_else(|| unbuilt(FillSide::Second, rb, lb, second(options)))?;
    }
    let face = face_path.ok_or_else(|| unbuilt(FillSide::Face, l0, r0, options.face))?;

    // the loop, clockwise: face, right wall down, bottom, left wall up
    let mut segs = face.clone();
    segs.extend(right_wall.iter().copied());
    segs.extend(bottom.clone());
    segs.extend(left_wall.iter().rev().map(Seg2::reversed));
    // the input check: no two sides of the loop cross. A face's own pieces are not checked
    // against each other (they are built to meet); each face is checked against the walls,
    // the bottom and the other face.
    let mut group = vec![1; face.len()];
    group.extend(right_wall.iter().map(|_| 0));
    group.extend(bottom.iter().map(|_| if second_used { 2 } else { 0 }));
    group.extend(left_wall.iter().map(|_| 0));
    if loop_crosses(&segs, &group, near_miss(eps)) {
        return Err(SolidError::invalid_input(
            OP,
            "a section that crosses itself (walls that cross)",
        ));
    }
    let region = Region2 {
        outer: Loop2 { segs }.reversed(),
        holes: Vec::new(),
    };
    // the loop's joins: within the section's allowance, or within the rounding of an arc's ends
    // where that is larger (a nearly straight arc of a large radius): an end read from its
    // centre, radius and angle lies within gamma(4) (|c| + r) + gamma(1) r |angle| of the exact
    // one in each coordinate, and an arc built through two points has its angles rounded too;
    // gamma(8) of the reach below covers both of a join's ends
    let reach = region
        .outer
        .segs
        .iter()
        .filter_map(|s| match *s {
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => Some(
                center.x.abs().max(center.y.abs()) + radius * (1.0 + start.abs() + sweep.abs()),
            ),
            _ => None,
        })
        .fold(0.0_f64, f64::max);
    let closure = eps.max(1e-12).max(gamma(8) * reach);
    if let Err(refusal) = check_regions(std::iter::once(&region), closure, zone) {
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
    // a concave face's dip: below the chord, and below each toe's level
    for (side, contour, closing, toes, out) in [
        (FillSide::Face, options.face, &face, [l0, r0], normal),
        (
            FillSide::Second,
            second(options),
            &bottom,
            [lb, rb],
            normal.map(|n| -n),
        ),
    ] {
        let used = side == FillSide::Face || (through && !cut);
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
        notes.push(FillNote::BelowToes {
            side,
            depth,
            below: below_toes,
            ratio: depth / channel_depth,
        });
    }
    Ok(Filled {
        region,
        channel_depth,
        filled_depth,
        through,
        notes,
    })
}

fn check_input(section: &Channel, options: &FillOptions) -> Result<()> {
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
        .depth_normal
        .is_some_and(|n| !(n.x.is_finite() && n.y.is_finite()) || (n.x == 0.0 && n.y == 0.0))
    {
        return Err(SolidError::invalid_input(
            OP,
            "a depth normal that is not finite and non-zero",
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
            "a depth that is not finite and positive",
        ));
    }
    Ok(())
}

fn finite(p: Point2) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

/// The size of the walls: their largest coordinate (an arc's points where it passes an axis
/// direction included), at least 1, and at least a millionth of any arc's radius. That floor
/// keeps the near-miss tolerance above the rounding of an arc's points: a point read as centre
/// plus radius along a direction is off by about 3 `f64::EPSILON` (|centre| + radius), and
/// |centre| is at most the extent plus the radius, so by at most 3 `f64::EPSILON` (extent +
/// 2 radius), while the tolerance is 1e-7 of the size, at least the larger of 1e-7 extent and
/// 1e-13 radius: some 75 times the rounding or more at every radius. Not
/// the centre's own coordinates: a shallow wall's centre lies far off, and the tolerances
/// would outgrow the section. Past a radius of 1e6 times the walls' extent the floor sets the
/// size, and the tolerances grow with the radius, as its rounding does.
fn scale<'a>(segs: impl Iterator<Item = &'a Seg2>) -> f64 {
    let axes = [
        Vec2::new(1.0, 0.0),
        Vec2::new(-1.0, 0.0),
        Vec2::new(0.0, 1.0),
        Vec2::new(0.0, -1.0),
    ];
    segs.fold(1.0_f64, |m, s| {
        let far = match *s {
            Seg2::Arc { radius, .. } => axes
                .iter()
                .flat_map(|&d| deepest_points(s, d))
                .fold(radius * 1e-6, |m, p| m.max(p.max_dimension())),
            _ => 0.0,
        };
        m.max(s.start().max_dimension())
            .max(s.end().max_dimension())
            .max(far)
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

/// A face closing the run from `p` to `q` with `contour`, its outside on the left of the run:
/// the chord, a sagitta arc, or the convex arc with blended toes (lines and arcs, joined in
/// order); `None` for a convex or concave arc past half a circle.
#[must_use]
pub fn close(p: Point2, q: Point2, contour: Contour) -> Option<Vec<Seg2>> {
    let chord = q - p;
    let c = chord.length();
    if !(c.is_finite() && c > 0.0) {
        return None;
    }
    let e = chord * (1.0 / c);
    let n = left_normal(e);
    let m = (p + q) * 0.5;
    let a = 0.5 * c;
    match contour {
        Contour::Flat => Some(vec![Seg2::Line { a: p, b: q }]),
        Contour::Convex { height } => sagitta_arc(p, q, height).map(|arc| vec![arc]),
        Contour::Concave { depth } => sagitta_arc(p, q, -depth).map(|arc| vec![arc]),
        Contour::ConvexBlended {
            height: h,
            overlap: o,
        } => {
            if o <= 0.0 {
                return sagitta_arc(p, q, h).map(|arc| vec![arc]);
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
            Some(vec![
                Seg2::Line { a: p, b: t_left },
                arc_about(c_left, t_left, i_left, true)?,
                arc_about(c_main, i_left, i_right, false)?,
                arc_about(c_right, i_right, t_right, true)?,
                Seg2::Line { a: t_right, b: q },
            ])
        }
    }
}

/// The arc from `p` to `q` whose middle stands `sagitta` off the chord, on the left of the run
/// when positive, on its right when negative: the polyline arc of bulge `-2 sagitta / chord`
/// (`prism::arc_conversion::bulge_arc`; a positive bulge turns counter-clockwise, bulging right
/// of the run). `None` past half a circle (`|sagitta|` more than half the chord).
fn sagitta_arc(p: Point2, q: Point2, sagitta: f64) -> Option<Seg2> {
    let bulge = -2.0 * sagitta / (q - p).length();
    bulge_arc(Vertex { at: p, bulge }, q)
}

/// A face too shallow for an arc: up to the zone tolerance it is built flat and a note says so.
/// Past it, a face lower than the smallest arc the profile builds (`floor`) does not build. A
/// convex face is refused here with that floor; a concave one is sized with it later, with
/// what it touches. The floor is the height at which the face's smallest arc sweeps more than
/// `smallest_arc_sweep` and its largest radius is below `MAX_ARC_RADIUS`: with `x = a + o`
/// (`o = 0` but for blended toes), the main arc sweeps `4 atan(h / x)` and each blend arc half
/// that, the main arc's radius is `a (x² + h²) / 2hx` and a blend arc's `o (x² + h²) / 2hx`:
/// the largest uses `m = max(a, o)`. The floor uses the sweep bound for
/// an infinite radius, dropping `smallest_arc_sweep`'s `zone / R` term: the sweep exceeds that
/// term for any `a > zone / 2`. The `(1 + 1e-9)` keeps a conservative allowance above it.
fn shallow(
    side: FillSide,
    p: Point2,
    q: Point2,
    contour: Contour,
    zone: f64,
    notes: &mut Vec<FillNote>,
) -> Result<(Contour, f64)> {
    let (size, overlap) = match contour {
        Contour::Convex { height } => (height, 0.0),
        Contour::ConvexBlended { height, overlap } => (height, overlap.max(0.0)),
        Contour::Concave { depth } => (depth, 0.0),
        Contour::Flat => return Ok((contour, 0.0)),
    };
    if size <= zone {
        notes.push(FillNote::BuiltFlat { side, asked: size });
        return Ok((Contour::Flat, 0.0));
    }
    let a = 0.5 * p.distance(q);
    let x = a + overlap;
    let m = a.max(overlap);
    let b = MAX_ARC_RADIUS;
    let turn = if overlap > 0.0 { 0.5 } else { 0.25 };
    let floor = (x * det::tan(turn * smallest_arc_sweep(f64::MAX, zone)))
        .max(x * m / (b + (b * b - m * m).max(0.0).sqrt()))
        * (1.0 + 1e-9);
    if size < floor && !matches!(contour, Contour::Concave { .. }) {
        return Err(
            Failure::new(OP, Reason::Fill(FillFailure::FaceTooShallow { side, size }))
                .with_fit(Fit::new(floor, FitMeasure::Distance, None))
                .into_error(),
        );
    }
    Ok((contour, floor))
}

/// Blended toes, before a face's heights are sized (`a` half the chord). An overlap no longer
/// than the zone tolerance is built with sharp toes and a note says so: the sharp face lies
/// within the zone tolerance of the blended one. (Its blend arcs would build only at some
/// heights: a blend arc is `o (1 + u²) atan(u) / u` long, `u = h / x`, from `o` to `π o / 2`,
/// so at or below `2 zone / π` at none.) A height the blended face allows, up to `a + o`, is
/// kept and built at most `a` high, the sharp face's limit, so sharp toes refuse no height
/// the blend would build; past it the sharp face's own limit refuses it. A longer overlap only
/// where the sides beyond the toes run along the chord (`along`), else refused whatever the
/// height (a height at or below the zone tolerance is built flat, and has no toes).
fn toes(
    side: FillSide,
    contour: Contour,
    a: f64,
    along: bool,
    (zone, eps): (f64, f64),
    notes: &mut Vec<FillNote>,
) -> Result<Contour> {
    let Contour::ConvexBlended { height, overlap } = contour else {
        return Ok(contour);
    };
    if height <= zone || overlap <= 0.0 {
        return Ok(contour);
    }
    if overlap <= zone {
        let built = if height <= a + overlap + eps {
            height.min(a)
        } else {
            height
        };
        notes.push(FillNote::SharpToes {
            side,
            overlap,
            asked: height,
            height: built,
        });
        return Ok(Contour::Convex { height: built });
    }
    if !along {
        return Err(
            Failure::new(OP, Reason::Fill(FillFailure::BlendedToesOffChord { side })).into_error(),
        );
    }
    Ok(contour)
}

/// A face's own limits, before it is built: a convex face no higher than half a circle allows
/// (its fit `a + o`, half the toe distance plus the blend's overlap: the main arc's half sweep
/// `2 atan(h / (a + o))` reaches a quarter turn there).
fn check_face(side: FillSide, p: Point2, q: Point2, contour: Contour, eps: f64) -> Result<()> {
    let a = 0.5 * p.distance(q);
    let (height, overlap) = match contour {
        Contour::Convex { height } => (height, 0.0),
        Contour::ConvexBlended { height, overlap } => (height, overlap.max(0.0)),
        Contour::Flat | Contour::Concave { .. } => return Ok(()),
    };
    let fit = a + overlap;
    if height > fit + eps {
        return Err(Failure::new(
            OP,
            Reason::Fill(FillFailure::ConvexFaceTooHigh { side, height }),
        )
        .with_fit(Fit::new(fit, FitMeasure::Distance, None))
        .into_error());
    }
    Ok(())
}

/// Why a face whose limits hold still has no polyline arcs (its arcs a rounding past half a
/// circle at the limit): a convex face is too high, a concave one too deep, with its fit.
fn unbuilt(side: FillSide, p: Point2, q: Point2, contour: Contour) -> SolidError {
    let a = 0.5 * p.distance(q);
    let (height, overlap) = match contour {
        Contour::Convex { height } => (height, 0.0),
        Contour::ConvexBlended { height, overlap } => (height, overlap.max(0.0)),
        Contour::Concave { depth } => {
            return Failure::new(
                OP,
                Reason::Fill(FillFailure::ConcaveFaceTooDeep { side, depth }),
            )
            .with_fit(Fit::new(a, FitMeasure::Distance, None))
            .into_error();
        }
        Contour::Flat => return SolidError::invalid_input(OP, "a face that does not build"),
    };
    Failure::new(
        OP,
        Reason::Fill(FillFailure::ConvexFaceTooHigh { side, height }),
    )
    .with_fit(Fit::new(a + overlap, FitMeasure::Distance, None))
    .into_error()
}

/// The deepest circular arc from `p` to `q` (dipping on the right of the run) that clears
/// `others`, lines and arcs: the depth where the arc first comes within the crossing check's
/// reach of one of them, and whether it does (`false`: none is touched up to the half circle,
/// half the chord, which is then the limit). `eps` is the conservative allowance of the
/// section the sides belong to (as for [`near_miss`]). Each side is grown towards the arcs by
/// twice the near-miss tolerance, so an arc short of the limit is never flagged by
/// [`loop_crosses`]; a side that leaves a toe (`p` or `q`) is grown the same way back past the
/// toe. A vertex of `others` between `eps` and that growth below the chord, between the toes,
/// leaves no arc clear of it: the limit is 0 (touched). A side that crosses the chord between
/// the toes and runs on below the half circle meets every arc and is not looked for: the
/// caller's to refuse (`fill`'s crossing check refuses such a section). It decides at that
/// tolerance, not at a proved bound; a caller keeps a depth margin below the limit. `p` and `q`
/// at one point (or not finite) give `(0, false)`; the sides are the caller's to pass finite.
/// Closed form: the arcs through `p` and `q` are a pencil of circles, each deeper one below the
/// one before, so the first that reaches a grown side does so at a vertex's circle or where it
/// is tangent to the side. The circle of dip `s` has its centre `m + n (R - s)`,
/// `R = (a² + s²) / 2s` (`m` the chord's middle, `a` half of it, `n` its normal on the
/// outside); tangency with a line or a circle is a quadratic in `s`.
#[must_use]
pub fn arc_clearance(p: Point2, q: Point2, others: &[Seg2], eps: f64) -> (f64, bool) {
    let chord = q - p;
    let a = 0.5 * chord.length();
    if !(a.is_finite() && a > 0.0) {
        return (0.0, false);
    }
    let e = chord * (0.5 / a);
    let n = left_normal(e);
    let m = (p + q) * 0.5;
    let circle = |s: f64| {
        let r = (a * a + s * s) / (2.0 * s);
        (m + n * (r - s), r)
    };
    // within the arcs' reach: below the chord, between the toes
    let inside = |x: Point2| (x - m).dot(n) <= eps && (x - m).dot(e).abs() <= a + eps;
    let tol = near_miss(eps);
    // every side grown towards the arcs by twice that: the crossing check flags a miss up to
    // it (an outside miss between circles up to 2 tol, line and inside misses up to tol)
    let off = 2.0 * tol;
    let (mut best, mut contact) = (a, false);
    let mut take = |s: f64| {
        if s >= 0.0 && s <= best {
            best = s;
            contact = true;
        }
    };
    let at_toe = |x: Point2| x.distance(p).min(x.distance(q)) <= tol;
    // |x - m|² - a², read as (x - p) . (x - q): no cancellation for a point near a toe
    let power = |x: Point2| (x - p).dot(x - q);
    for seg in others {
        let (s0, s1) = (seg.start(), seg.end());
        // a side leaving a toe at the angle t: the arc leaves the toe 2 atan(s / a) below the
        // chord, so it reaches the side's line at the toe at s = a tan(t / 2), short of which
        // its circle meets that line again only behind the toe. Like every side it is grown
        // `off` towards the arcs, here back past the toe: the arc stops at the circle through
        // the side's line `off` behind the toe (a vertex's circle, below). The crossing check
        // reads that second meeting by deflation from the toe, to a few eps_f64 R, so it tells
        // it from the toe at any radius.
        for (toe, other) in [(p, q), (q, p)] {
            let Some(dir) = leaving(seg, toe, tol) else {
                continue;
            };
            let toward = (other - toe) * (1.0 / (2.0 * a));
            let angle = det::atan2(-dir.dot(n), dir.dot(toward));
            if angle > 0.0 && angle < PI {
                // read from the step back past the toe, not from the point's coordinates (whose
                // rounding is large beside a step of `off`)
                let back = -(dir * off);
                let wn = (m - toe).dot(n) - back.dot(n);
                let toe_power = (toe - other + back).dot(back);
                let mut found = false;
                for s in roots(-wn, toe_power, wn * a * a) {
                    if s > eps && s <= a + eps {
                        take(s);
                        found = true;
                    }
                }
                if !found {
                    // a side all but along the chord: the touch itself
                    take(a * det::tan(0.5 * angle));
                }
            }
        }
        for x in [s0, s1] {
            let v = x - m;
            let under = v.dot(n);
            if x.distance(p).min(x.distance(q)) <= eps || under >= -eps || v.dot(e).abs() >= a {
                continue;
            }
            // a vertex within `off` of the chord between the toes: no arc clears it (a channel
            // shallower than the tolerances, such as a wall at a tiny angle): the limit is flat
            if under > -off {
                take(0.0);
                continue;
            }
            // the first arc within `off` of the vertex: centre distance R + off
            let wn = (m - x).dot(n);
            for s in roots(-wn - off, power(x) - off * off, (wn - off) * a * a) {
                if s > eps && s <= a + eps {
                    take(s);
                }
            }
        }
        match *seg {
            Seg2::Line { a: l0, b: l1 } => {
                let d = l1 - l0;
                let len = d.length();
                if len <= eps {
                    continue;
                }
                let t = d * (1.0 / len);
                let ln = left_normal(t);
                let (g, k) = ((m - l0).dot(ln), n.dot(ln));
                for sigma in [1.0, -1.0] {
                    // the line moved `off` towards the arc: g + k (R - s) = sigma (R + off),
                    // that is (g - sigma off) + k (R - s) = sigma R, times 2s
                    let g = g - sigma * off;
                    for s in roots(-k - sigma, 2.0 * g, (k - sigma) * a * a) {
                        if s <= eps || s > a + eps {
                            continue;
                        }
                        let (c, _) = circle(s);
                        let x = c - ln * (c - l0).dot(ln);
                        let u = (x - l0).dot(t);
                        if (-tol..=len + tol).contains(&u) && inside(x) && !at_toe(x) {
                            take(s);
                        }
                    }
                }
            }
            Seg2::Arc { center, radius, .. } => {
                let w = m - center;
                let wn = w.dot(n);
                // the side grown towards the arc: outside it, its radius grows by `off`
                // (|c - center| = R + radius + off); inside it, the side's circle grows if the
                // arc's circle holds it (R - radius - off) and shrinks if it holds the arc's
                // circle (radius - off - R). Never shrunk otherwise: that would add roots.
                for (sigma, grown, holds) in [
                    (1.0, radius + off, None),
                    (-1.0, radius + off, Some(false)),
                    (-1.0, radius - off, Some(true)),
                ] {
                    // |c - center| = R + sigma grown, squared, times 2s
                    for s in roots(
                        -wn - sigma * grown,
                        w.dot(w) - a * a - grown * grown,
                        (wn - sigma * grown) * a * a,
                    ) {
                        if s <= eps || s > a + eps || grown <= 0.0 {
                            continue;
                        }
                        let (c, r) = circle(s);
                        if holds.is_some_and(|h| h != (radius > r)) {
                            continue;
                        }
                        let span = c - center;
                        let dist = span.length();
                        if dist <= eps {
                            continue;
                        }
                        let u = span * (1.0 / dist);
                        let x = [center + u * radius, center - u * radius]
                            .into_iter()
                            .min_by(|x, y| {
                                (x.distance(c) - r)
                                    .abs()
                                    .total_cmp(&(y.distance(c) - r).abs())
                            })
                            .unwrap_or(center);
                        if on_seg(seg, x, tol) && inside(x) && !at_toe(x) {
                            take(s);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (best, contact)
}

/// The unit direction a side runs in from `toe`, where it starts or ends there.
fn leaving(seg: &Seg2, toe: Point2, tol: f64) -> Option<Vec2> {
    let (start, end) = (
        seg.start().distance(toe) <= tol,
        seg.end().distance(toe) <= tol,
    );
    if !(start || end) {
        return None;
    }
    let d = match *seg {
        Seg2::Line { a, b } => {
            if start {
                b - a
            } else {
                a - b
            }
        }
        Seg2::Arc {
            start: s, sweep, ..
        } => {
            let at = if start { s } else { s + sweep };
            let travel = Vec2::new(-det::sin(at), det::cos(at)) * sweep.signum();
            if start {
                travel
            } else {
                -travel
            }
        }
        _ => return None,
    };
    let len = d.length();
    (len > 0.0).then(|| d * (1.0 / len))
}

/// The real roots of `a x² + b x + c` (one of `b x + c` when `a` is zero).
fn roots(a: f64, b: f64, c: f64) -> Vec<f64> {
    if a.abs() <= 1e-15 * (b.abs() + c.abs()) {
        return if b == 0.0 { Vec::new() } else { vec![-c / b] };
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return Vec::new();
    }
    // the root without cancellation, and the other from the product
    let r = -0.5 * (b + b.signum() * disc.sqrt());
    if r == 0.0 {
        return vec![0.0];
    }
    vec![r / a, c / r]
}

/// The arc about `center` from `p` to `q`, counter-clockwise when `ccw`, as the polyline arc
/// of its bulge `tan(sweep / 4)`; `None` past half a circle.
fn arc_about(center: Point2, p: Point2, q: Point2, ccw: bool) -> Option<Seg2> {
    let (dp, dq) = (p - center, q - center);
    let turn = (det::atan2(dq.y, dq.x) - det::atan2(dp.y, dp.x)).rem_euclid(TAU);
    let sweep = if ccw { turn } else { turn - TAU };
    bulge_arc(
        Vertex {
            at: p,
            bulge: det::tan(sweep / 4.0),
        },
        q,
    )
}

/// The chain of lines and arcs `chain` up to where it first reaches `s` below the line through
/// `origin` square to `up` (a unit vector), ending there; `None` when it never does. `eps` is
/// the conservative allowance: a chain that reaches `s` within it of its start is `None`, one
/// that reaches it within it of a segment's end ends at that end. The caller passes finite
/// segments and a unit `up` (`s` is a distance only along a unit vector).
#[must_use]
pub fn cut_chain(chain: &[Seg2], s: f64, origin: Point2, up: Vec2, eps: f64) -> Option<Vec<Seg2>> {
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
                    let base = det::atan2(up.y, up.x);
                    let off = det::acos(k);
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
        // the cut's distance from the segment's ends along it, against eps (a distance)
        let len = match *seg {
            Seg2::Line { a, b } => a.distance(b),
            Seg2::Arc { radius, sweep, .. } => radius * sweep.abs(),
            _ => 0.0,
        };
        match hit {
            Some(t) if t * len <= eps => {
                return (!out.is_empty()).then_some(out);
            }
            Some(t) if (1.0 - t) * len > eps => {
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
