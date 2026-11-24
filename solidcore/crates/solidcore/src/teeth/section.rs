//! Tooth outlines as profiles a caller sweeps into tools: a whole outline as one closed loop of
//! profile segments, the section a cutter takes out of one tooth space where it leaves a shaft,
//! and the section of a chamfer's tool at the end of a round face with the plane it is drawn in.
//!
//! A profile here is the kernel's own ([`Seg2`]): lines, arcs, cubic and rational quadratic
//! Bezier pieces and Bezier pieces of higher degree. A spline of an outline becomes its Bezier
//! pieces, and where two pieces of one curve meet is given with the profile ("joints"), so that a
//! caller can sweep a run of them into one face.
//!
//! Not upstream: SolidCraft's shaft spline, a kernel primitive because it computes geometry any
//! client could use (docs/layers.md §1). No recursion. Inputs are hostile: a count of teeth no
//! outline has, a curve that is not a spline, numbers that are not finite give a refusal or a
//! plain answer, never a panic.

use std::f64::consts::{PI, TAU};

use crate::error::teeth::{PlanRefusal, ProfileWhy, TeethFailure, Why};
use crate::eval::Frame;
use crate::math::Vec2;
use crate::prism::profile::{Seg2, MAX_BEZIER_CTRL};
use crate::teeth::outline::{Piece, Point, Role};
use crate::teeth::seat::normal;
use crate::teeth::{fit, MAX_TEETH};
use crate::{SolidError, Vec3};

/// The widest stretch of an arc one cubic piece stands for in a cutter's section (radians, thirty
/// degrees): the piece is then within four ten-millionths of the arc's radius of the arc, and an
/// arc up to that wide is one face (a face is split where pieces of a curve join with only their
/// directions in common).
pub const ARC_SPAN: f64 = PI / 6.0;

/// The widest tooth space (radians, 170 degrees) that ends in the cutter's curve: the section's
/// walls stand on the space's two corners along its middle line, which a space near half a turn
/// wide has no room for.
pub const MOST_SPACE: f64 = 170.0 * PI / 180.0;

/// How far the pieces that stand for an arc in a cutter's section are bent off it, as a share of
/// the size of the work (one and its largest coordinate): thirty times the share within which a
/// turned curve is taken for an arc (`FIT_REL` of the analytic revolution, a thousand-millionth),
/// so the bend is seen, and far below anything a part is made to (a few millionths of a
/// millimetre on a part of a hundred millimetres).
pub const ARC_BEND: f64 = 3e-8;

/// A piece of an outline as it lies in the whole loop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoopPiece {
    /// An arc about the axis on the outline's largest circle.
    pub top: bool,
    /// What the piece is.
    pub role: Role,
    /// The angle of its middle where it is a top; else 0, turned with its tooth.
    pub middle: f64,
    /// The angle of its start, where its first segment is a free-form curve that starts on the
    /// largest circle.
    pub corner: Option<f64>,
    /// The first of the loop's segments that are its.
    pub first: usize,
    /// How many of the loop's segments are its.
    pub segs: usize,
}

/// A whole outline as one closed loop of profile segments.
#[derive(Clone, Debug, PartialEq)]
pub struct OutlineLoop {
    /// The segments, counter-clockwise.
    pub segs: Vec<Seg2>,
    /// Where two segments of one curve meet.
    pub joints: Vec<Vec2>,
    /// The pieces of the outline, in the loop's order.
    pub pieces: Vec<LoopPiece>,
}

/// The section a cutter takes out of one tooth space where it leaves the shaft.
#[derive(Clone, Debug, PartialEq)]
pub struct CutterSection {
    /// The floor's spans in the floor's order, then three lines: the wall above the tooth before
    /// the space, the top, the wall above the tooth after it.
    pub segs: Vec<Seg2>,
    /// How many of `segs` each piece of the space's floor became, in the floor's order (0 for a
    /// piece too short to draw).
    pub spans: Vec<usize>,
    /// The angle of the space's middle line.
    pub middle: f64,
    /// The corner of the tooth before the space, as it lies with the middle line on angle 0.
    pub from: [f64; 2],
    /// The corner of the tooth after the space, as it lies with the middle line on angle 0.
    pub to: [f64; 2],
    /// Where two spans of one curve meet.
    pub joints: Vec<Vec2>,
    /// A point of the cutter's axis in the section's plane.
    pub axis_at: Vec2,
    /// The direction of the cutter's axis in the section's plane.
    pub axis_dir: Vec2,
}

/// A chamfer at an end of a round face: the corner it takes off lies at `radius`, `past` beyond
/// the face's own end (the body's own end, where that lies further out).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EndChamfer {
    /// The radius of the round face: the tops of a shaft's teeth, of a hub's.
    pub radius: f64,
    /// How far the chamfer runs across the end.
    pub radial: f64,
    /// How far the chamfer runs along the face.
    pub axial: f64,
    /// How far past the face's end the body's own end lies.
    pub past: f64,
    /// At a step, where the body runs on past the end: the tool stops at the end, and at this
    /// radius (a shaft's neck) where the chamfer would run deeper than the step.
    pub flush: Option<f64>,
}

/// The section of a chamfer's tool in a plane through the axis, counter-clockwise, and which of
/// its sides is the slope.
#[derive(Clone, Debug, PartialEq)]
pub struct ChamferSection {
    /// The corners of the section.
    pub points: Vec<Vec2>,
    /// The side from `points[slope]` to the next corner is the slope.
    pub slope: usize,
}

/// A refusal of a profile, in the planner's words.
fn not_a_profile(why: ProfileWhy) -> TeethFailure {
    TeethFailure::Refused(PlanRefusal::NotAProfile(why))
}

/// A point of a tooth outline as a profile point.
fn v2(p: Point) -> Vec2 {
    Vec2::new(p[0], p[1])
}

/// One Bezier piece, given by its control points, as a profile segment.
fn bezier_seg(points: &[Point]) -> Result<Seg2, TeethFailure> {
    Ok(match points {
        [a, b] => Seg2::Line {
            a: v2(*a),
            b: v2(*b),
        },
        [a, apex, b] => Seg2::Conic {
            a: v2(*a),
            apex: v2(*apex),
            b: v2(*b),
            w: 1.0,
        },
        [p0, p1, p2, p3] => Seg2::Cubic {
            p0: v2(*p0),
            p1: v2(*p1),
            p2: v2(*p2),
            p3: v2(*p3),
        },
        _ if (5..=MAX_BEZIER_CTRL).contains(&points.len()) => {
            let mut p = [Vec2::ZERO; MAX_BEZIER_CTRL];
            for (slot, q) in p.iter_mut().zip(points) {
                *slot = v2(*q);
            }
            let n = u8::try_from(points.len() - 1)
                .map_err(|_| not_a_profile(ProfileWhy::DegreeTooHigh))?;
            Seg2::Bezier { n, p }
        }
        _ => return Err(not_a_profile(ProfileWhy::DegreeTooHigh)),
    })
}

/// The profile segments of a piece of an outline added to `out`, with the points where two
/// segments of one curve meet added to `joints`. A piece too short to draw gives none.
fn piece_segs(
    piece: &Piece,
    out: &mut Vec<Seg2>,
    joints: &mut Vec<Point>,
) -> Result<(), TeethFailure> {
    match piece {
        Piece::Line { from, to, .. } => {
            if (from[0] - to[0]).hypot(from[1] - to[1])
                > 1e-12 * (1.0 + from[0].abs() + from[1].abs())
            {
                out.push(Seg2::Line {
                    a: v2(*from),
                    b: v2(*to),
                });
            }
        }
        Piece::Arc {
            centre,
            radius,
            start,
            sweep,
            ..
        } => {
            if *radius > 1e-9 && sweep.abs() > 1e-12 {
                out.push(Seg2::Arc {
                    center: v2(*centre),
                    radius: *radius,
                    start: *start,
                    sweep: *sweep,
                });
            }
        }
        Piece::Spline {
            degree,
            knots,
            control,
            ..
        } => {
            let pieces = fit::bezier_pieces_typed(*degree, knots, control).map_err(|e| {
                not_a_profile(ProfileWhy::Spline(e.why().unwrap_or(Why::NotASpline)))
            })?;
            for (k, points) in pieces.iter().enumerate() {
                if k > 0 {
                    if let Some(&joint) = points.first() {
                        joints.push(joint);
                    }
                }
                out.push(bezier_seg(points)?);
            }
        }
        Piece::Curve { .. } => return Err(not_a_profile(ProfileWhy::NotFitted)),
    }
    Ok(())
}

/// A profile segment turned about the origin by the angle whose sine and cosine are `s` and `c`.
fn turned_seg(seg: &Seg2, s: f64, c: f64) -> Seg2 {
    let t = |p: Vec2| Vec2::new(p.x * c - p.y * s, p.x * s + p.y * c);
    match *seg {
        Seg2::Line { a, b } => Seg2::Line { a: t(a), b: t(b) },
        Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        } => Seg2::Arc {
            center: t(center),
            radius,
            start: start + s.atan2(c),
            sweep,
        },
        Seg2::Cubic { p0, p1, p2, p3 } => Seg2::Cubic {
            p0: t(p0),
            p1: t(p1),
            p2: t(p2),
            p3: t(p3),
        },
        Seg2::Conic { a, apex, b, w } => Seg2::Conic {
            a: t(a),
            apex: t(apex),
            b: t(b),
            w,
        },
        Seg2::Bezier { n, p } => Seg2::Bezier { n, p: p.map(t) },
    }
}

/// The first point of a free-form segment.
fn seg_start(s: &Seg2) -> Option<Vec2> {
    match *s {
        Seg2::Cubic { p0, .. } => Some(p0),
        Seg2::Conic { a, .. } => Some(a),
        Seg2::Bezier { p, .. } => p.first().copied(),
        Seg2::Line { .. } | Seg2::Arc { .. } => None,
    }
}

/// The whole outline of `teeth` teeth, the pieces of one `tooth` repeated every `pitch` radians,
/// as one closed loop, every tooth turned to its place and the whole turned by `turn`; `top` is
/// the outline's largest radius. With it, where two segments of one curve meet, and each piece as
/// it lies in the loop ([`LoopPiece`]).
///
/// Refused: a count of teeth no outline has (none, or more than [`MAX_TEETH`]), a curve held
/// only as points, a spline that is not a B-spline, and a Bezier piece of more control points than
/// a profile segment holds.
pub fn outline_loop(
    teeth: u32,
    pitch: f64,
    tooth: &[Piece],
    top: f64,
    turn: f64,
) -> crate::Result<OutlineLoop> {
    outline_loop_typed(teeth, pitch, tooth, top, turn)
        .map_err(|e| SolidError::teeth("teeth::section::outline_loop", e))
}

/// [`outline_loop`] with its failure as a [`TeethFailure`].
fn outline_loop_typed(
    teeth: u32,
    pitch: f64,
    tooth: &[Piece],
    top: f64,
    turn: f64,
) -> Result<OutlineLoop, TeethFailure> {
    if teeth == 0 || teeth > MAX_TEETH {
        return Err(TeethFailure::OutOfRange {
            what: "teeth",
            value: f64::from(teeth),
            above: 0.0,
            up_to: f64::from(MAX_TEETH),
        });
    }
    let mut template = Vec::new();
    let mut points = Vec::new();
    // For each piece of one tooth: whether it is a top, what it is, the angle of its middle, its
    // segments.
    let mut one: Vec<LoopPiece> = Vec::new();
    for piece in tooth {
        let first = template.len();
        let arc_on_top = match piece {
            Piece::Arc {
                centre,
                radius,
                start,
                sweep,
                ..
            } if centre[0].hypot(centre[1]) <= 1e-9 * (1.0 + top)
                && (radius - top).abs() <= 1e-6 * (1.0 + top)
                && sweep.abs() > 1e-12 =>
            {
                Some((*start, *sweep))
            }
            _ => None,
        };
        piece_segs(piece, &mut template, &mut points)?;
        // Where the piece starts, when that is a point of the largest circle.
        let corner = template
            .get(first)
            .and_then(seg_start)
            .filter(|p| (p.x.hypot(p.y) - top).abs() <= 1e-6 * (1.0 + top))
            .map(|p| p.y.atan2(p.x));
        one.push(LoopPiece {
            top: arc_on_top.is_some(),
            role: piece.role(),
            middle: arc_on_top.map_or(0.0, |(start, sweep)| start + sweep / 2.0),
            corner,
            first,
            segs: template.len() - first,
        });
    }
    let mut segs = Vec::new();
    let mut pieces: Vec<LoopPiece> = Vec::new();
    let mut joints = Vec::new();
    for k in 0..teeth {
        let by = turn + pitch * f64::from(k);
        let (s, c) = by.sin_cos();
        let start = segs.len();
        segs.extend(template.iter().map(|g| turned_seg(g, s, c)));
        pieces.extend(one.iter().map(|p| LoopPiece {
            top: p.top,
            role: p.role,
            middle: p.middle + by,
            corner: p.corner.map(|at| at + by),
            first: start + p.first,
            segs: p.segs,
        }));
        for p in &points {
            let q = v2(*p);
            joints.push(Vec2::new(q.x * c - q.y * s, q.x * s + q.y * c));
        }
    }
    Ok(OutlineLoop {
        segs,
        joints,
        pieces,
    })
}

/// An arc as cubic pieces of at most [`ARC_SPAN`] each, each bent off the arc by `bend` (its two
/// inner points moved along the arc's radius, one out and one in, so it runs a little outside the
/// arc and then a little inside). A cutter's section holds no arc, and nothing taken for one: a
/// curve taken for an arc, turned about a line, is a torus, and the torus of a short arc is
/// refused (its numbers are worked back out of its spline form, which a short arc does not fix
/// closely enough).
///
/// The pieces of one arc carry on from one another without a corner (where one piece ends bent
/// in, the next starts bent out by as much), and the points where they meet are added to
/// `joints`: the arc is one curve, so one face.
fn arc_cubics(
    centre: Point,
    radius: f64,
    start: f64,
    sweep: f64,
    bend: f64,
    out: &mut Vec<Seg2>,
    joints: &mut Vec<Point>,
) {
    let spans = (sweep.abs() / ARC_SPAN).ceil().clamp(1.0, 64.0) as u32; // 1 to 64
    let step = sweep / f64::from(spans);
    let reach = 4.0 / 3.0 * (step / 4.0).tan() * radius;
    // A point of the arc, the direction of travel there and the direction out along the radius.
    let at = |j: u32| {
        let (s, c) = (start + step * f64::from(j)).sin_cos();
        (
            [centre[0] + radius * c, centre[1] + radius * s],
            [-s, c],
            [c, s],
        )
    };
    for j in 0..spans {
        let ((a, da, na), (b, db, nb)) = (at(j), at(j + 1));
        if j + 1 < spans {
            joints.push(b);
        }
        out.push(Seg2::Cubic {
            p0: v2(a),
            p1: Vec2::new(
                a[0] + reach * da[0] + bend * na[0],
                a[1] + reach * da[1] + bend * na[1],
            ),
            p2: Vec2::new(
                b[0] - reach * db[0] - bend * nb[0],
                b[1] - reach * db[1] - bend * nb[1],
            ),
            p3: v2(b),
        });
    }
}

/// The section a cutter takes out of one tooth space where it leaves the shaft, on the plane where
/// the full depth ends: the space's own sides and bottom (`floor`: from `after`, the corner of the
/// tooth after the space, down its side, along the bottom and up to `before`, the corner of the
/// tooth before), then from those corners two walls along the space's middle line up to `out_to`
/// from the shaft's axis, and a top square to that line. Turned about the cutter's axis the walls
/// are flat, the top a cylinder, straight sides cones and each curve a surface of its own; no
/// piece is an arc (an arc of the floor becomes bent cubic pieces). With it: the angle of the
/// middle line, the two corners as they lie with that line on angle 0, and the cutter's axis in
/// the section's plane, square to the middle line at `root` plus `cutter_radius` from the shaft's
/// axis.
///
/// The bend of the arcs' pieces is [`ARC_BEND`] times one and the size of the work: the largest
/// absolute coordinate of `plane_origin` (the origin of the section's plane), the root radius and
/// the cutter's radius.
///
/// Refused: a space as wide as [`MOST_SPACE`] or more (or whose corners give no width), and a
/// floor that cannot be made into a profile.
pub fn cutter_section(
    floor: &[Piece],
    after: Point,
    before: Point,
    root: f64,
    cutter_radius: f64,
    out_to: f64,
    plane_origin: Vec3,
) -> crate::Result<CutterSection> {
    cutter_section_typed(
        floor,
        after,
        before,
        root,
        cutter_radius,
        out_to,
        plane_origin,
    )
    .map_err(|e| SolidError::teeth("teeth::section::cutter_section", e))
}

/// [`cutter_section`] with its failure as a [`TeethFailure`].
fn cutter_section_typed(
    floor: &[Piece],
    after: Point,
    before: Point,
    root: f64,
    cutter_radius: f64,
    out_to: f64,
    plane_origin: Vec3,
) -> Result<CutterSection, TeethFailure> {
    // The size of the work: the largest coordinate the section and the cutter's axis reach.
    let reach = plane_origin
        .x
        .abs()
        .max(plane_origin.y.abs())
        .max(plane_origin.z.abs())
        + root
        + cutter_radius;
    let bend = ARC_BEND * (1.0 + reach);
    let from = before[1].atan2(before[0]);
    let wide = (after[1].atan2(after[0]) - from).rem_euclid(TAU);
    if !(wide > 0.0 && wide < MOST_SPACE) {
        return Err(TeethFailure::Refused(PlanRefusal::CutterSpaceTooWide));
    }
    let middle = from + 0.5 * wide;
    let (s, c) = middle.sin_cos();
    let mut segs = Vec::new();
    let mut spans = Vec::new();
    let mut points = Vec::new();
    for piece in floor {
        let before_piece = segs.len();
        match piece {
            Piece::Arc {
                centre,
                radius,
                start,
                sweep,
                ..
            } => {
                if *radius > 1e-9 && sweep.abs() > 1e-12 {
                    arc_cubics(
                        *centre,
                        *radius,
                        *start,
                        *sweep,
                        bend,
                        &mut segs,
                        &mut points,
                    );
                }
            }
            _ => piece_segs(piece, &mut segs, &mut points)?,
        }
        spans.push(segs.len() - before_piece);
    }
    let joints = points.iter().map(|q| v2(*q)).collect();
    // Up from a corner along the middle line to `out_to` from the axis.
    let up = |p: Point| {
        let rise = out_to - (p[0] * c + p[1] * s);
        Vec2::new(p[0] + rise * c, p[1] + rise * s)
    };
    let (high_before, high_after) = (up(before), up(after));
    segs.push(Seg2::Line {
        a: v2(before),
        b: high_before,
    });
    segs.push(Seg2::Line {
        a: high_before,
        b: high_after,
    });
    segs.push(Seg2::Line {
        a: high_after,
        b: v2(after),
    });
    // The corners as they lie with the middle line on angle 0: equal spaces are equal there.
    let local = |p: Point| [p[0] * c + p[1] * s, p[1] * c - p[0] * s];
    Ok(CutterSection {
        segs,
        spans,
        middle,
        from: local(before),
        to: local(after),
        joints,
        axis_at: Vec2::new((root + cutter_radius) * c, (root + cutter_radius) * s),
        axis_dir: Vec2::new(-s, c),
    })
}

/// The section of a chamfer's tool in a plane through the axis (x across from the axis, y along
/// the face from its end), counter-clockwise, and which of its sides is the slope. On a shaft
/// (`outside`) the tool takes the corner off the end, in from the radius; in a bore it widens the
/// entry, out from the radius. It reaches `margin` past the end and past the face, but for a step,
/// where it stops at the end. None where no such section lies clear of the axis, or the numbers
/// are not finite.
#[must_use]
pub fn chamfer_section(c: &EndChamfer, margin: f64, outside: bool) -> Option<ChamferSection> {
    if !(c.radial > 0.0
        && c.axial > 0.0
        && c.radius > 0.0
        && c.radial.is_finite()
        && c.axial.is_finite()
        && c.past.is_finite())
    {
        return None;
    }
    let more = margin / c.radial.min(c.axial);
    let sign = if outside { 1.0 } else { -1.0 };
    // The slope's two ends: on the end's plane, and on the face.
    let on_end = Vec2::new(c.radius - sign * c.radial, -c.past);
    let on_face = Vec2::new(c.radius, -c.past + c.axial);
    let along = on_face - on_end;
    let top = on_face + along * more;
    let low = if c.flush.is_some() {
        on_end
    } else {
        on_end - along * more
    };
    let corner = Vec2::new(top.x, low.y);
    // Where the body runs on past the end nearer than the slope comes to, the tool stops at its
    // surface.
    let stop = c.flush.filter(|neck| {
        if outside {
            *neck > low.x
        } else {
            *neck < low.x
        }
    });
    let (points, slope) = match (stop, outside) {
        (None, true) => (vec![low, corner, top], 2),
        (None, false) => (vec![corner, low, top], 1),
        (Some(neck), true) => {
            if neck >= c.radius {
                return None;
            }
            (
                vec![
                    Vec2::new(neck, low.y),
                    corner,
                    top,
                    Vec2::new(neck, low.y + c.axial * (neck - low.x) / c.radial),
                ],
                2,
            )
        }
        (Some(neck), false) => {
            if neck <= c.radius {
                return None;
            }
            (
                vec![
                    corner,
                    Vec2::new(neck, low.y),
                    Vec2::new(neck, low.y + c.axial * (low.x - neck) / c.radial),
                    top,
                ],
                2,
            )
        }
    };
    points
        .iter()
        .all(|p| p.x > 0.0 && p.x.is_finite() && p.y.is_finite())
        .then_some(ChamferSection { points, slope })
}

/// The plane through the axis a chamfer's section is drawn in: the origin of `plane` (the plane
/// square to the axis at the face's end), x across the shaft at the angle `seam` from the plane's
/// x towards its y, y along the axis into the face (the plane's normal times `inward`).
#[must_use]
pub fn chamfer_plane(plane: &Frame, inward: f64, seam: f64) -> Frame {
    let (s, c) = seam.sin_cos();
    let (x, y) = (
        Vec3::from_array(plane.x_axis),
        Vec3::from_array(plane.y_axis),
    );
    Frame {
        origin: plane.origin,
        x_axis: (x * c + y * s).to_array(),
        y_axis: (normal(plane) * inward).to_array(),
    }
}
