//! Teeth with straight flanks: straight-sided splines and serrations, shafts and the hubs that fit
//! them.
//!
//! Every piece of these outlines is exact: the flanks are straight lines ([`Piece::Line`]), the
//! tips, the roots and the rounds and chamfers between them are circular arcs ([`Piece::Arc`]) and
//! straight lines.
//!
//! A tooth is laid out from one line, its upper flank, and two circles about the axis, the major
//! and the minor circle. The lower flank is the upper one mirrored in the tooth's middle line (the
//! x axis). Where the flank meets the major circle the corner is left sharp, cut by a chamfer, or
//! rounded by a round that touches both; where it meets the minor circle it is left sharp or
//! rounded the same way.
//!
//! - A **straight-sided spline** ([`StraightSpline`]) has flanks parallel to the tooth's middle
//!   line, half the tooth's width from it.
//! - A **serration** ([`Serration`]) has many small teeth whose two flanks meet at a stated angle.
//!   Its flank crosses the mean of the major and minor circles a quarter of a pitch from the middle
//!   of the tooth, so that tooth and space are equally wide there. That is a plain construction,
//!   not a standard's.
//!
//! The hub's opening is laid out the same way: its spaces take the shaft's teeth, with the flank
//! moved away by the side clearance, the bottom of the space beyond the shaft's major circle by the
//! major clearance, and the hub's teeth short of the shaft's minor circle by the minor clearance.
//! [`hub_for`] and [`serration_hub_for`] derive the hub and refuse corners that would come closer
//! to the shaft than the smallest of the three clearances. [`turning_play`] and [`serration_play`]
//! give how far the hub turns on the shaft before the flanks touch.
//!
//! Lengths are millimetres, the flank angle of a serration degrees, angles of an outline radians.
//! No proportions here are taken from a standard.

use std::f64::consts::{PI, TAU};

use crate::error::teeth::{TeethFailure, Why};
use crate::teeth::mesh::Material;
use crate::teeth::outline::{checked_tolerance, rotated, Outline, Piece, Point, Role};
use crate::teeth::pair::{Motion, Pair, Part};
use crate::teeth::{in_range, zero_up_to, MAX_LENGTH, MAX_TEETH};
use crate::SolidError;

// -------------------------------------------------------------------------------------------------
// Plane geometry
// -------------------------------------------------------------------------------------------------

fn add(a: Point, b: Point) -> Point {
    [a[0] + b[0], a[1] + b[1]]
}

fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1]]
}

fn scaled(a: Point, k: f64) -> Point {
    [a[0] * k, a[1] * k]
}

fn dot(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn cross(a: Point, b: Point) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn length(a: Point) -> f64 {
    a[0].hypot(a[1])
}

fn angle_of(a: Point) -> f64 {
    a[1].atan2(a[0])
}

fn mirrored(p: Point) -> Point {
    [p[0], -p[1]]
}

/// An angle brought into the range from minus a half turn to a half turn.
fn wrapped(angle: f64) -> f64 {
    let a = (angle + PI).rem_euclid(TAU) - PI;
    if a <= -PI {
        a + TAU
    } else {
        a
    }
}

/// A line or an arc turned about the origin. Other pieces are returned as they are (this module
/// makes none).
pub(super) fn turned_piece(piece: &Piece, angle: f64) -> Piece {
    match piece {
        Piece::Line { from, to, role } => Piece::Line {
            from: rotated(*from, angle),
            to: rotated(*to, angle),
            role: *role,
        },
        Piece::Arc {
            centre,
            radius,
            start,
            sweep,
            role,
        } => Piece::Arc {
            centre: rotated(*centre, angle),
            radius: *radius,
            start: start + angle,
            sweep: *sweep,
            role: *role,
        },
        other => other.clone(),
    }
}

/// The pieces of the teeth `-2` to `2` of an outline (all of them when it has fewer than five
/// teeth): enough to find every contact of one tooth of another part that turns by less than a
/// pitch.
pub(super) fn pieces_near(outline: &Outline) -> Vec<Piece> {
    let teeth = outline.teeth();
    let turns: Vec<i64> = if teeth < 5 {
        (0..i64::from(teeth)).collect()
    } else {
        (-2..=2).collect()
    };
    let pitch = outline.pitch_angle();
    turns
        .into_iter()
        .flat_map(|k| {
            outline
                .tooth()
                .iter()
                .map(move |piece| turned_piece(piece, pitch * k as f64))
        })
        .collect()
}

/// Whether `q`, a point on the circle or the line of `piece`, lies within the piece, to `within`
/// millimetres.
fn within_piece(piece: &Piece, q: Point, within: f64) -> bool {
    match piece {
        Piece::Line { from, to, .. } => {
            let along = sub(*to, *from);
            let long = length(along);
            if !(long > 0.0) {
                return length(sub(q, *from)) <= within;
            }
            let t = dot(sub(q, *from), along) / long;
            t >= -within && t <= long + within
        }
        Piece::Arc {
            centre,
            radius,
            start,
            sweep,
            ..
        } => {
            if !(*radius > within) {
                return length(sub(q, *centre)) <= radius + within;
            }
            let slack = within / radius;
            let into = ((angle_of(sub(q, *centre)) - start) * sweep.signum()).rem_euclid(TAU);
            into <= sweep.abs() + slack || into >= TAU - slack
        }
        _ => false,
    }
}

/// The turns (radians, from 0 up to a whole turn) by which a point `p` turned about the origin in
/// the sense `sense` (1 counter-clockwise, -1 clockwise) comes to an angle `alpha` that satisfies
/// `cos(alpha - towards) = ratio`.
fn turns_to(p: Point, sense: f64, towards: f64, ratio: f64) -> Vec<f64> {
    if !(ratio.abs() <= 1.0 + 1e-12) {
        return Vec::new();
    }
    let half = ratio.clamp(-1.0, 1.0).acos();
    let from = angle_of(p);
    [half, -half]
        .into_iter()
        .map(|x| {
            let turn = (sense * (towards + x - from)).rem_euclid(TAU);
            if turn > TAU - 1e-12 {
                0.0
            } else {
                turn
            }
        })
        .collect()
}

/// Turns at which a point turned about the origin comes onto the line of points `q` with `q .
/// normal = offset`.
fn onto_line(p: Point, sense: f64, normal: Point, offset: f64) -> Vec<f64> {
    let r = length(p);
    if !(r > 0.0) {
        return Vec::new();
    }
    turns_to(p, sense, angle_of(normal), offset / r)
}

/// Turns at which a point turned about the origin comes onto the circle of `centre` and `radius`.
/// None for a circle about the origin: the turning point keeps its distance from it.
fn onto_circle(p: Point, sense: f64, centre: Point, radius: f64) -> Vec<f64> {
    let (r, c) = (length(p), length(centre));
    if !(r > 0.0 && c > 1e-12 * (1.0 + r)) {
        return Vec::new();
    }
    turns_to(
        p,
        sense,
        angle_of(centre),
        (r * r + c * c - radius * radius) / (2.0 * r * c),
    )
}

/// The line of a straight piece as its unit normal (to the right of the direction of travel) and
/// offset.
fn line_of(from: Point, to: Point) -> Option<(Point, f64)> {
    let along = sub(to, from);
    let long = length(along);
    if !(long > 0.0) {
        return None;
    }
    let normal = [along[1] / long, -along[0] / long];
    Some((normal, dot(normal, from)))
}

/// The smallest turn, from 0 up to `limit` radians, by which the part `turning` can be turned
/// counter-clockwise about the origin before it touches the part `fixed`; none if it does not touch
/// within `limit`.
///
/// Both parts are lines and arcs. Two outlines that turn into contact first meet where a corner of
/// one reaches the other, or where a round of one comes to touch a line or a round of the other.
/// Each such event is a point on a circle about the origin reaching a line or a circle, which has
/// an exact answer; the events whose point of contact lies on both pieces are the contacts, and the
/// first of them is the answer. Pieces that turn on themselves (arcs about the origin) never come
/// into contact by turning and are passed over.
pub(super) fn first_touch(fixed: &[Piece], turning: &[Piece], limit: f64) -> Option<f64> {
    let size = fixed
        .iter()
        .chain(turning)
        .flat_map(|p| [p.start(), p.end()])
        .flatten()
        .map(length)
        .fold(1.0, f64::max);
    let within = 1e-9 * size;
    let mut best: Option<f64> = None;
    let mut keep = |turn: f64| {
        if turn <= limit && best.is_none_or(|b| turn < b) {
            best = Some(turn);
        }
    };
    let corners = |pieces: &[Piece]| -> Vec<Point> {
        pieces
            .iter()
            .flat_map(|p| [p.start(), p.end()])
            .flatten()
            .collect()
    };
    // A corner of one part reaching a piece of the other, the other standing still: a corner of the
    // turning part turns forwards, a corner of the fixed part backwards as seen from the turning
    // part.
    for (corners, pieces, sense) in [
        (corners(turning), fixed, 1.0),
        (corners(fixed), turning, -1.0),
    ] {
        for &corner in &corners {
            for piece in pieces {
                let turns = match piece {
                    Piece::Line { from, to, .. } => line_of(*from, *to)
                        .map_or(Vec::new(), |(n, k)| onto_line(corner, sense, n, k)),
                    Piece::Arc { centre, radius, .. } => {
                        onto_circle(corner, sense, *centre, *radius)
                    }
                    _ => Vec::new(),
                };
                for turn in turns {
                    if within_piece(piece, rotated(corner, sense * turn), within) {
                        keep(turn);
                    }
                }
            }
        }
    }
    // A round of the turning part coming to touch a line or a round of the fixed part.
    for round in turning {
        let Piece::Arc {
            centre: ct,
            radius: e,
            ..
        } = round
        else {
            continue;
        };
        if !(length(*ct) > within && *e > 0.0) {
            continue;
        }
        for piece in fixed {
            match piece {
                Piece::Line { from, to, .. } => {
                    let Some((n, k)) = line_of(*from, *to) else {
                        continue;
                    };
                    for turn in [k + e, k - e]
                        .into_iter()
                        .flat_map(|level| onto_line(*ct, 1.0, n, level))
                    {
                        let at = rotated(*ct, turn);
                        let q = sub(at, scaled(n, dot(n, at) - k));
                        if within_piece(piece, q, within)
                            && within_piece(round, rotated(q, -turn), within)
                        {
                            keep(turn);
                        }
                    }
                }
                Piece::Arc {
                    centre: cf,
                    radius: d,
                    ..
                } => {
                    for (reach, outward) in [(d + e, true), ((d - e).abs(), d > e)] {
                        if !(reach > 0.0) {
                            continue;
                        }
                        for turn in onto_circle(*ct, 1.0, *cf, reach) {
                            let at = rotated(*ct, turn);
                            let apart = length(sub(at, *cf));
                            if !(apart > 0.0) {
                                continue;
                            }
                            let toward =
                                scaled(sub(at, *cf), if outward { d / apart } else { -d / apart });
                            let q = add(*cf, toward);
                            if within_piece(piece, q, within)
                                && within_piece(round, rotated(q, -turn), within)
                            {
                                keep(turn);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    // A round of the fixed part coming to touch a line of the turning part.
    for round in fixed {
        let Piece::Arc {
            centre: cf,
            radius: d,
            ..
        } = round
        else {
            continue;
        };
        if !(length(*cf) > within && *d > 0.0) {
            continue;
        }
        for line in turning {
            let Piece::Line { from, to, .. } = line else {
                continue;
            };
            let Some((n, k)) = line_of(*from, *to) else {
                continue;
            };
            for turn in [k + d, k - d]
                .into_iter()
                .flat_map(|level| onto_line(*cf, -1.0, n, level))
            {
                let at = rotated(*cf, -turn);
                let q = sub(at, scaled(n, dot(n, at) - k));
                if within_piece(line, q, within) && within_piece(round, rotated(q, turn), within) {
                    keep(turn);
                }
            }
        }
    }
    best
}

/// The turning play of a pair that turns as one on one axis: how far the second part turns
/// counter-clockwise from where it stands before it touches the first.
pub(super) fn play_of(pair: &Pair) -> Result<f64, TeethFailure> {
    let fixed = pieces_near(&pair.first.outline);
    let turning: Vec<Piece> = pieces_near(&pair.second.outline)
        .iter()
        .map(|p| turned_piece(p, pair.second_start))
        .collect();
    first_touch(&fixed, &turning, pair.cycle.abs().min(TAU))
        .ok_or(TeethFailure::Invalid(Why::TurnsFreely))
}

// -------------------------------------------------------------------------------------------------
// One tooth from a flank line and two circles
// -------------------------------------------------------------------------------------------------

/// How the corner where a flank meets the major circle of a shaft is finished.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TipCorner {
    /// Left sharp: the flank runs into the major circle.
    Plain,
    /// Cut by a straight chamfer at 45 degrees to the flank, starting `size` millimetres down the
    /// flank from the sharp corner.
    Chamfer {
        /// How far down the flank the chamfer starts, from the sharp corner, in millimetres.
        size: f64,
    },
    /// Rounded by a round of `radius` millimetres that touches the flank and the major circle.
    Round {
        /// The radius of the round.
        radius: f64,
    },
}

/// The finish of the corner where a flank meets the outer circle of a profile.
#[derive(Clone, Copy, Debug)]
enum Corner {
    Plain,
    Chamfer(f64),
    Round(f64),
}

impl Corner {
    fn of(tip: TipCorner) -> Result<Corner, TeethFailure> {
        Ok(match tip {
            TipCorner::Plain => Corner::Plain,
            TipCorner::Chamfer { size } => match zero_up_to("chamfer size", size, MAX_LENGTH)? {
                0.0 => Corner::Plain,
                size => Corner::Chamfer(size),
            },
            TipCorner::Round { radius } => {
                match zero_up_to("tip round radius", radius, MAX_LENGTH)? {
                    0.0 => Corner::Plain,
                    radius => Corner::Round(radius),
                }
            }
        })
    }

    fn round(radius: f64) -> Corner {
        if radius > 0.0 {
            Corner::Round(radius)
        } else {
            Corner::Plain
        }
    }
}

/// What the pieces of a profile are on the part, and what to say when a profile cannot be made.
#[derive(Clone, Copy, Debug)]
struct Kind {
    outer_arc: Role,
    outer_corner: Role,
    inner_arc: Role,
    inner_round: Role,
    outer_meet: Why,
    inner_meet: Why,
}

const SHAFT: Kind = Kind {
    outer_arc: Role::Tip,
    outer_corner: Role::Tip,
    inner_arc: Role::Root,
    inner_round: Role::Fillet,
    outer_meet: Why::TipCornersMeet,
    inner_meet: Why::TeethMeetAtRoot,
};

const HUB: Kind = Kind {
    outer_arc: Role::Root,
    outer_corner: Role::Fillet,
    inner_arc: Role::Tip,
    inner_round: Role::Tip,
    outer_meet: Why::HubRootRoundsMeet,
    inner_meet: Why::HubTeethTooNarrow,
};

/// A tooth (or, in a hub, a space) between two circles about the axis, centred on the x axis. Its
/// upper flank is the line of points `q` with `q . normal = offset`, `normal` pointing away from
/// the tooth.
#[derive(Clone, Copy, Debug)]
struct Profile {
    teeth: u32,
    normal: Point,
    offset: f64,
    outer: f64,
    inner: f64,
    outer_corner: Corner,
    inner_round: f64,
    kind: Kind,
}

/// The outer corner of a profile, laid out.
#[derive(Clone, Copy, Debug)]
enum OuterLaid {
    Plain,
    Chamfer { on_flank: Point, on_circle: Point },
    Round { centre: Point, radius: f64 },
}

/// The upper flank of a profile and its corners, laid out.
#[derive(Clone, Copy, Debug)]
struct Laid {
    /// Where the flank meets the outer and the inner circle, sharp.
    outer_apex: Point,
    inner_apex: Point,
    /// Where the straight flank ends at the outer and at the inner corner.
    flank_outer: Point,
    flank_inner: Point,
    /// The angle, from the x axis, at which the outer arc ends and the inner arc begins.
    outer_half: f64,
    inner_half: f64,
    outer: OuterLaid,
    /// The centre of the inner round, if there is one.
    inner_centre: Option<Point>,
}

impl Profile {
    /// Along the flank, towards the axis.
    fn inward(&self) -> Point {
        [-self.normal[1], self.normal[0]]
    }

    /// Where the line `q . normal = level` (parallel to the flank) first meets the circle of
    /// `radius` about the axis, coming in from outside along the tooth.
    fn crossing(&self, level: f64, radius: f64) -> Option<Point> {
        if !(radius > level.abs()) {
            return None;
        }
        let along = -(radius * radius - level * level).sqrt();
        Some(add(
            scaled(self.normal, level),
            scaled(self.inward(), along),
        ))
    }

    fn laid(&self) -> Result<Laid, TeethFailure> {
        let (c, n, w) = (self.offset, self.normal, self.inward());
        let pitch = TAU / f64::from(self.teeth.max(1));
        let missing = TeethFailure::Invalid(Why::FlankMissesCircles);
        let outer_apex = self.crossing(c, self.outer).ok_or(missing)?;
        let inner_apex = self.crossing(c, self.inner).ok_or(missing)?;
        let (flank_outer, outer_half, outer) = match self.outer_corner {
            Corner::Plain => (outer_apex, angle_of(outer_apex), OuterLaid::Plain),
            Corner::Chamfer(size) => {
                let on_flank = add(outer_apex, scaled(w, size));
                if !(length(on_flank) < self.outer) {
                    return Err(TeethFailure::Invalid(Why::ChamferTooLarge));
                }
                let way = scaled(sub([0.0, 0.0], add(w, n)), std::f64::consts::FRAC_1_SQRT_2);
                let (b, k) = (
                    dot(on_flank, way),
                    dot(on_flank, on_flank) - self.outer * self.outer,
                );
                let on_circle = add(on_flank, scaled(way, -b + (b * b - k).max(0.0).sqrt()));
                (
                    on_flank,
                    angle_of(on_circle),
                    OuterLaid::Chamfer {
                        on_flank,
                        on_circle,
                    },
                )
            }
            Corner::Round(radius) => {
                let centre = self
                    .crossing(c - radius, self.outer - radius)
                    .filter(|_| radius < self.outer)
                    .ok_or(TeethFailure::Invalid(Why::MajorRoundTooLarge))?;
                (
                    add(centre, scaled(n, radius)),
                    angle_of(centre),
                    OuterLaid::Round { centre, radius },
                )
            }
        };
        let (flank_inner, inner_half, inner_centre) = if self.inner_round > 0.0 {
            let rho = self.inner_round;
            let centre = self
                .crossing(c + rho, self.inner + rho)
                .ok_or(TeethFailure::Invalid(Why::MinorRoundTooLarge))?;
            (sub(centre, scaled(n, rho)), angle_of(centre), Some(centre))
        } else {
            (inner_apex, angle_of(inner_apex), None)
        };
        if !(outer_half >= -1e-12) {
            return Err(TeethFailure::Invalid(self.kind.outer_meet));
        }
        if !(inner_half <= pitch / 2.0 + 1e-12 && outer_half < pitch / 2.0) {
            return Err(TeethFailure::Invalid(self.kind.inner_meet));
        }
        if !(dot(flank_inner, w) - dot(flank_outer, w) >= -1e-12 * self.outer) {
            return Err(TeethFailure::Invalid(Why::NoStraightFlank));
        }
        Ok(Laid {
            outer_apex,
            inner_apex,
            flank_outer,
            flank_inner,
            outer_half: outer_half.max(0.0),
            inner_half: inner_half.min(pitch / 2.0),
            outer,
            inner_centre,
        })
    }

    /// The pieces of one pitch, counter-clockwise: lower flank, lower outer corner, outer arc,
    /// upper outer corner, upper flank, upper inner round, inner arc, and the inner round of the
    /// next tooth's lower flank.
    fn pieces(&self) -> Result<Vec<Piece>, TeethFailure> {
        let laid = self.laid()?;
        let kind = self.kind;
        let pitch = TAU / f64::from(self.teeth.max(1));
        let tiny = 1e-12 * (1.0 + self.outer);
        let mut tooth = Vec::new();
        let line = |tooth: &mut Vec<Piece>, from: Point, to: Point, role: Role| {
            if length(sub(to, from)) > tiny {
                tooth.push(Piece::Line { from, to, role });
            }
        };
        line(
            &mut tooth,
            mirrored(laid.flank_inner),
            mirrored(laid.flank_outer),
            Role::Flank,
        );
        let up = angle_of(self.normal);
        let round_sweep = |centre: Point| wrapped(up - angle_of(centre));
        match laid.outer {
            OuterLaid::Plain => {}
            OuterLaid::Chamfer {
                on_flank,
                on_circle,
            } => {
                line(
                    &mut tooth,
                    mirrored(on_flank),
                    mirrored(on_circle),
                    kind.outer_corner,
                );
            }
            OuterLaid::Round { centre, radius } => {
                tooth.push(Piece::Arc {
                    centre: mirrored(centre),
                    radius,
                    start: -up,
                    sweep: round_sweep(centre),
                    role: kind.outer_corner,
                });
            }
        }
        if laid.outer_half > 1e-15 {
            tooth.push(Piece::Arc {
                centre: [0.0, 0.0],
                radius: self.outer,
                start: -laid.outer_half,
                sweep: 2.0 * laid.outer_half,
                role: kind.outer_arc,
            });
        }
        match laid.outer {
            OuterLaid::Plain => {}
            OuterLaid::Chamfer {
                on_flank,
                on_circle,
            } => line(&mut tooth, on_circle, on_flank, kind.outer_corner),
            OuterLaid::Round { centre, radius } => {
                tooth.push(Piece::Arc {
                    centre,
                    radius,
                    start: angle_of(centre),
                    sweep: round_sweep(centre),
                    role: kind.outer_corner,
                });
            }
        }
        line(&mut tooth, laid.flank_outer, laid.flank_inner, Role::Flank);
        let inner_round = laid.inner_centre.map(|centre| {
            let start = angle_of(scaled(self.normal, -1.0));
            (
                centre,
                start,
                wrapped(angle_of(scaled(centre, -1.0)) - start),
            )
        });
        if let Some((centre, start, sweep)) = inner_round {
            tooth.push(Piece::Arc {
                centre,
                radius: self.inner_round,
                start,
                sweep,
                role: kind.inner_round,
            });
        }
        let inner_sweep = pitch - 2.0 * laid.inner_half;
        if inner_sweep > 1e-15 {
            tooth.push(Piece::Arc {
                centre: [0.0, 0.0],
                radius: self.inner,
                start: laid.inner_half,
                sweep: inner_sweep,
                role: kind.inner_arc,
            });
        }
        if let Some((centre, start, sweep)) = inner_round {
            tooth.push(Piece::Arc {
                centre: rotated(mirrored(centre), pitch),
                radius: self.inner_round,
                start: -start - sweep + pitch,
                sweep,
                role: kind.inner_round,
            });
        }
        Ok(tooth)
    }

    fn outline(&self, tolerance: f64) -> Result<Outline, TeethFailure> {
        let tolerance = checked_tolerance(tolerance)?;
        Outline::new_typed(self.teeth, self.pieces()?, tolerance)
    }
}

/// The circles about which the outer corner of a part is made: the points, each with its own
/// radius, whose circles together make up the corner (a sharp corner is a circle of no radius at
/// its point).
fn outer_circles(laid: &Laid) -> Vec<(Point, f64)> {
    match laid.outer {
        OuterLaid::Plain => vec![(laid.outer_apex, 0.0)],
        OuterLaid::Chamfer {
            on_flank,
            on_circle,
        } => vec![(on_flank, 0.0), (on_circle, 0.0)],
        OuterLaid::Round { centre, radius } => vec![(centre, radius)],
    }
}

/// Whether a hub keeps at least `gap` from its shaft at the two corners of a flank.
///
/// At the outer corner the shaft's corner, grown by the gap, must stay within the hub's space: each
/// circle of the shaft's corner, grown by the gap, must lie inside the space. Where the space's
/// corner is rounded, a circle that sits in the corner, beyond the two lines through the round's
/// centre at right angles to the flank and to the major circle, lies inside the space when it lies
/// inside the round. At the inner corner the same holds for the hub's tooth in the shaft's space.
fn corners_keep(
    shaft: &Profile,
    shaft_laid: &Laid,
    hub: &Profile,
    hub_laid: &Laid,
    gap: f64,
) -> Result<(), TeethFailure> {
    let (n, w) = (shaft.normal, shaft.inward());
    let slack = 1e-9 * (1.0 + hub.outer);
    for (q, own) in outer_circles(shaft_laid) {
        let need = own + gap;
        let mut fits =
            dot(q, n) <= hub.offset - need + slack && length(q) <= hub.outer - need + slack;
        if let OuterLaid::Round { centre, radius } = hub_laid.outer {
            if radius > need {
                let in_corner = dot(sub(q, centre), w) <= 0.0 && cross(centre, q) >= 0.0;
                fits &= !in_corner || length(sub(q, centre)) <= radius - need + slack;
            }
        }
        if !fits {
            return Err(TeethFailure::Invalid(Why::HubRootRoundsTooClose));
        }
    }
    let (q, own) = (
        hub_laid.inner_centre.unwrap_or(hub_laid.inner_apex),
        hub.inner_round,
    );
    let need = own + gap;
    let mut fits =
        dot(q, n) >= shaft.offset + need - slack && length(q) >= shaft.inner + need - slack;
    if let Some(centre) = shaft_laid.inner_centre {
        if shaft.inner_round > need {
            let in_corner = dot(sub(q, centre), w) >= 0.0 && cross(centre, q) <= 0.0;
            fits &= !in_corner || length(sub(q, centre)) <= shaft.inner_round - need + slack;
        }
    }
    if !fits {
        return Err(TeethFailure::Invalid(Why::HubTipsTooClose));
    }
    Ok(())
}

fn checked_teeth(teeth: u32) -> Result<u32, TeethFailure> {
    if (2..=MAX_TEETH).contains(&teeth) {
        Ok(teeth)
    } else {
        Err(TeethFailure::OutOfRange {
            what: "teeth",
            value: f64::from(teeth),
            above: 1.0,
            up_to: f64::from(MAX_TEETH),
        })
    }
}

/// The major and minor radius from two diameters, the major the larger.
fn radii(major_diameter: f64, minor_diameter: f64) -> Result<(f64, f64), TeethFailure> {
    let major = in_range("major diameter", major_diameter, 0.0, MAX_LENGTH)? / 2.0;
    let minor = in_range("minor diameter", minor_diameter, 0.0, MAX_LENGTH)? / 2.0;
    if !(minor < major) {
        return Err(TeethFailure::Invalid(Why::MinorNotSmaller));
    }
    Ok((major, minor))
}

/// How a hub fits its shaft, for straight-sided splines and serrations alike.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    /// The gap between each flank of a shaft's tooth and the hub's flank beside it when the hub is
    /// centred, measured at right angles to the flanks, in millimetres.
    pub side_clearance: f64,
    /// The gap between the shaft's major circle and the bottom of the hub's spaces, in millimetres.
    pub major_clearance: f64,
    /// The gap between the tips of the hub's teeth and the shaft's minor circle, in millimetres.
    pub minor_clearance: f64,
    /// The radius of the rounds at the bottom corners of the hub's spaces, in millimetres (0 for
    /// sharp corners).
    pub hub_root_round: f64,
    /// The radius of the rounds at the tips of the hub's teeth, in millimetres (0 for sharp
    /// corners).
    pub hub_tip_round: f64,
}

/// The three clearances of a fit, checked, and the smallest of them.
fn clearances(fit: &Fit) -> Result<(f64, f64, f64, f64), TeethFailure> {
    let side = zero_up_to("side clearance", fit.side_clearance, MAX_LENGTH)?;
    let major = zero_up_to("major clearance", fit.major_clearance, MAX_LENGTH)?;
    let minor = zero_up_to("minor clearance", fit.minor_clearance, MAX_LENGTH)?;
    zero_up_to("hub root round", fit.hub_root_round, MAX_LENGTH)?;
    zero_up_to("hub tip round", fit.hub_tip_round, MAX_LENGTH)?;
    Ok((side, major, minor, side.min(major).min(minor)))
}

/// The hub's profile for a shaft's profile and a fit, checked against the shaft.
fn hub_profile(shaft: &Profile, fit: &Fit) -> Result<Profile, TeethFailure> {
    let (side, major, minor, least) = clearances(fit)?;
    let hub = Profile {
        teeth: shaft.teeth,
        normal: shaft.normal,
        offset: shaft.offset + side,
        outer: shaft.outer + major,
        inner: shaft.inner + minor,
        outer_corner: Corner::round(fit.hub_root_round),
        inner_round: fit.hub_tip_round,
        kind: HUB,
    };
    if !(hub.inner < shaft.outer) {
        return Err(TeethFailure::Invalid(Why::MinorClearanceTooLarge));
    }
    let (shaft_laid, hub_laid) = (shaft.laid()?, hub.laid()?);
    corners_keep(shaft, &shaft_laid, &hub, &hub_laid, least)?;
    Ok(hub)
}

fn pair_of(shaft: &Profile, hub: &Profile, tolerance: f64) -> Result<Pair, TeethFailure> {
    Ok(Pair {
        first: Part {
            outline: shaft.outline(tolerance)?,
            material: Material::Inside,
            axis: [0.0, 0.0],
        },
        second: Part {
            outline: hub.outline(tolerance)?,
            material: Material::Outside,
            axis: [0.0, 0.0],
        },
        motion: Motion::Turning { ratio: 1.0 },
        second_start: 0.0,
        cycle: TAU / f64::from(shaft.teeth),
    })
}

/// The turning play of a hub on its shaft from the two profiles. The tolerance only cuts the arcs
/// into points for the outline's own use; the play comes from the exact pieces.
fn play_of_profiles(shaft: &Profile, hub: &Profile) -> Result<f64, TeethFailure> {
    play_of(&pair_of(shaft, hub, 1e-3 * (1.0 + shaft.outer))?)
}

// -------------------------------------------------------------------------------------------------
// Straight-sided splines
// -------------------------------------------------------------------------------------------------

/// A shaft with straight-sided teeth: each tooth has two flanks parallel to its middle line,
/// `tooth_width` apart, and stands between the minor and the major circle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StraightSpline {
    /// How many teeth, 2 to [`MAX_TEETH`].
    pub teeth: u32,
    /// The diameter over the tips of the teeth, in millimetres.
    pub major_diameter: f64,
    /// The diameter at the bottom of the spaces between the teeth, in millimetres.
    pub minor_diameter: f64,
    /// The width of a tooth between its two flanks, in millimetres.
    pub tooth_width: f64,
    /// The radius of the rounds where the flanks meet the minor circle, in millimetres (0 for sharp
    /// corners).
    pub root_round: f64,
    /// How the corners at the tips are finished.
    pub tip: TipCorner,
}

impl StraightSpline {
    fn profile(&self) -> Result<Profile, TeethFailure> {
        let teeth = checked_teeth(self.teeth)?;
        let (outer, inner) = radii(self.major_diameter, self.minor_diameter)?;
        let width = in_range("tooth width", self.tooth_width, 0.0, MAX_LENGTH)?;
        let profile = Profile {
            teeth,
            normal: [0.0, 1.0],
            offset: width / 2.0,
            outer,
            inner,
            outer_corner: Corner::of(self.tip)?,
            inner_round: zero_up_to("root round", self.root_round, MAX_LENGTH)?,
            kind: SHAFT,
        };
        profile.laid()?;
        Ok(profile)
    }

    /// The outline of the shaft, a tooth centred on the x axis: all exact lines and arcs. The
    /// `tolerance` (millimetres) only says how finely [`Outline::points`] cuts the arcs.
    pub fn outline(&self, tolerance: f64) -> crate::Result<Outline> {
        self.outline_typed(tolerance)
            .map_err(|e| SolidError::teeth("teeth::straight::StraightSpline::outline", e))
    }

    /// [`Self::outline`] with its failure as a [`TeethFailure`], for the tooth module's own
    /// callers.
    pub(crate) fn outline_typed(&self, tolerance: f64) -> Result<Outline, TeethFailure> {
        self.profile()?.outline(tolerance)
    }
}

/// The opening of a hub for a straight-sided spline: spaces with parallel flanks `space_width`
/// apart between the hub's minor circle (the tips of its teeth) and its major circle (the bottom of
/// its spaces).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StraightHub {
    /// How many spaces, 2 to [`MAX_TEETH`].
    pub teeth: u32,
    /// The diameter at the bottom of the spaces, in millimetres.
    pub major_diameter: f64,
    /// The diameter over the tips of the hub's teeth, in millimetres.
    pub minor_diameter: f64,
    /// The width of a space between its two flanks, in millimetres.
    pub space_width: f64,
    /// The radius of the rounds at the bottom corners of the spaces, in millimetres (0 for sharp
    /// corners).
    pub root_round: f64,
    /// The radius of the rounds at the tips of the hub's teeth, in millimetres (0 for sharp
    /// corners).
    pub tip_round: f64,
}

impl StraightHub {
    fn profile(&self) -> Result<Profile, TeethFailure> {
        let teeth = checked_teeth(self.teeth)?;
        let (outer, inner) = radii(self.major_diameter, self.minor_diameter)?;
        let width = in_range("space width", self.space_width, 0.0, MAX_LENGTH)?;
        let profile = Profile {
            teeth,
            normal: [0.0, 1.0],
            offset: width / 2.0,
            outer,
            inner,
            outer_corner: Corner::round(zero_up_to("root round", self.root_round, MAX_LENGTH)?),
            inner_round: zero_up_to("tip round", self.tip_round, MAX_LENGTH)?,
            kind: HUB,
        };
        profile.laid()?;
        Ok(profile)
    }

    /// The outline of the hub's opening, a space centred on the x axis: all exact lines and arcs.
    pub fn outline(&self, tolerance: f64) -> crate::Result<Outline> {
        self.outline_typed(tolerance)
            .map_err(|e| SolidError::teeth("teeth::straight::StraightHub::outline", e))
    }

    /// [`Self::outline`] with its failure as a [`TeethFailure`], for the tooth module's own
    /// callers.
    pub(crate) fn outline_typed(&self, tolerance: f64) -> Result<Outline, TeethFailure> {
        self.profile()?.outline(tolerance)
    }
}

/// The hub that fits a straight-sided spline: spaces wider than the teeth by the side clearance on
/// each side, their bottoms beyond the shaft's major circle by the major clearance, and the hub's
/// teeth short of the shaft's minor circle by the minor clearance.
///
/// Refused when the hub cannot be made (its spaces would meet, its rounds do not fit) or when its
/// rounds would come closer to the shaft than the smallest of the three clearances.
pub fn hub_for(shaft: &StraightSpline, fit: &Fit) -> crate::Result<StraightHub> {
    hub_for_typed(shaft, fit).map_err(|e| SolidError::teeth("teeth::straight::hub_for", e))
}

/// [`hub_for`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
pub(crate) fn hub_for_typed(
    shaft: &StraightSpline,
    fit: &Fit,
) -> Result<StraightHub, TeethFailure> {
    let hub = hub_profile(&shaft.profile()?, fit)?;
    Ok(StraightHub {
        teeth: shaft.teeth,
        major_diameter: 2.0 * hub.outer,
        minor_diameter: 2.0 * hub.inner,
        space_width: 2.0 * hub.offset,
        root_round: fit.hub_root_round,
        tip_round: fit.hub_tip_round,
    })
}

/// A straight-sided spline and the hub that fits it ([`hub_for`]) as a pair on one axis. They turn
/// as one; adding to [`Pair::second_start`] turns the hub on the shaft. The smallest gap between
/// them is the smallest of the three clearances.
pub fn spline_pair(shaft: &StraightSpline, fit: &Fit, tolerance: f64) -> crate::Result<Pair> {
    spline_pair_typed(shaft, fit, tolerance)
        .map_err(|e| SolidError::teeth("teeth::straight::spline_pair", e))
}

/// [`spline_pair`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
pub(crate) fn spline_pair_typed(
    shaft: &StraightSpline,
    fit: &Fit,
    tolerance: f64,
) -> Result<Pair, TeethFailure> {
    let profile = shaft.profile()?;
    pair_of(&profile, &hub_profile(&profile, fit)?, tolerance)
}

/// How far, in radians, the hub of [`spline_pair`] turns on its shaft either way from centred
/// before a flank touches: found exactly from the lines and arcs of the two outlines.
pub fn turning_play(shaft: &StraightSpline, fit: &Fit) -> crate::Result<f64> {
    turning_play_typed(shaft, fit)
        .map_err(|e| SolidError::teeth("teeth::straight::turning_play", e))
}

/// [`turning_play`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
pub(crate) fn turning_play_typed(shaft: &StraightSpline, fit: &Fit) -> Result<f64, TeethFailure> {
    let profile = shaft.profile()?;
    play_of_profiles(&profile, &hub_profile(&profile, fit)?)
}

// -------------------------------------------------------------------------------------------------
// Serrations
// -------------------------------------------------------------------------------------------------

/// A serrated shaft: many small teeth with straight flanks that meet at `flank_angle`.
///
/// The flank crosses the circle midway between the major and the minor circle a quarter of a pitch
/// from the middle of the tooth, so that tooth and space are equally wide there. The tips are flats
/// on the major circle, with sharp, chamfered or rounded corners; the roots are flats on the minor
/// circle with sharp or rounded corners.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Serration {
    /// How many teeth, 2 to [`MAX_TEETH`].
    pub teeth: u32,
    /// The diameter over the tips, in millimetres.
    pub major_diameter: f64,
    /// The diameter at the roots, in millimetres.
    pub minor_diameter: f64,
    /// The angle between the two flanks of a tooth, in degrees, above 0 and at most 170.
    pub flank_angle: f64,
    /// How the corners at the tips are finished.
    pub tip: TipCorner,
    /// The radius of the rounds where the flanks meet the minor circle, in millimetres (0 for sharp
    /// corners).
    pub root_round: f64,
}

impl Serration {
    fn profile(&self) -> Result<Profile, TeethFailure> {
        let teeth = checked_teeth(self.teeth)?;
        let (outer, inner) = radii(self.major_diameter, self.minor_diameter)?;
        let half = in_range("flank angle", self.flank_angle, 0.0, 170.0)?.to_radians() / 2.0;
        // The flank turned by half the flank angle from the tooth's middle line, through the point
        // of the mean circle a quarter of a pitch from the middle of the tooth.
        let normal = [half.sin(), half.cos()];
        let quarter = PI / (2.0 * f64::from(teeth));
        let mean = (outer + inner) / 2.0;
        let offset = dot(normal, [mean * quarter.cos(), mean * quarter.sin()]);
        let profile = Profile {
            teeth,
            normal,
            offset,
            outer,
            inner,
            outer_corner: Corner::of(self.tip)?,
            inner_round: zero_up_to("root round", self.root_round, MAX_LENGTH)?,
            kind: SHAFT,
        };
        profile.laid()?;
        Ok(profile)
    }

    /// The outline of the shaft, a tooth centred on the x axis: all exact lines and arcs. The
    /// `tolerance` (millimetres) only says how finely [`Outline::points`] cuts the arcs.
    pub fn outline(&self, tolerance: f64) -> crate::Result<Outline> {
        self.outline_typed(tolerance)
            .map_err(|e| SolidError::teeth("teeth::straight::Serration::outline", e))
    }

    /// [`Self::outline`] with its failure as a [`TeethFailure`], for the tooth module's own
    /// callers.
    pub(crate) fn outline_typed(&self, tolerance: f64) -> Result<Outline, TeethFailure> {
        self.profile()?.outline(tolerance)
    }

    /// Where the two flanks of a tooth, carried on, would meet: the radius of the sharp point of a
    /// tooth.
    pub fn sharp_tip_radius(&self) -> crate::Result<f64> {
        self.sharp_tip_radius_typed()
            .map_err(|e| SolidError::teeth("teeth::straight::Serration::sharp_tip_radius", e))
    }

    /// [`Self::sharp_tip_radius`] with its failure as a [`TeethFailure`], for the tooth module's
    /// own callers.
    pub(crate) fn sharp_tip_radius_typed(&self) -> Result<f64, TeethFailure> {
        let p = self.profile()?;
        // On the x axis: q = (x, 0) with x * normal.x = offset.
        if !(p.normal[0] > 0.0) {
            return Err(TeethFailure::Invalid(Why::FlanksParallel));
        }
        let radius = p.offset / p.normal[0];
        if !(radius.is_finite() && radius <= MAX_LENGTH) {
            return Err(TeethFailure::Invalid(Why::FlanksNearlyParallel));
        }
        Ok(radius)
    }
}

/// The hub that fits a serrated shaft: the shaft and the fit it was derived with
/// ([`serration_hub_for`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SerratedHub {
    /// The shaft the hub fits.
    pub shaft: Serration,
    /// How it fits.
    pub fit: Fit,
}

impl SerratedHub {
    /// The outline of the hub's opening, a space centred on the x axis: all exact lines and arcs.
    pub fn outline(&self, tolerance: f64) -> crate::Result<Outline> {
        self.outline_typed(tolerance)
            .map_err(|e| SolidError::teeth("teeth::straight::SerratedHub::outline", e))
    }

    /// [`Self::outline`] with its failure as a [`TeethFailure`], for the tooth module's own
    /// callers.
    pub(crate) fn outline_typed(&self, tolerance: f64) -> Result<Outline, TeethFailure> {
        hub_profile(&self.shaft.profile()?, &self.fit)?.outline(tolerance)
    }

    /// The diameter at the bottom of the hub's spaces, in millimetres.
    pub fn major_diameter(&self) -> crate::Result<f64> {
        self.major_diameter_typed()
            .map_err(|e| SolidError::teeth("teeth::straight::SerratedHub::major_diameter", e))
    }

    /// [`Self::major_diameter`] with its failure as a [`TeethFailure`], for the tooth module's own
    /// callers.
    pub(crate) fn major_diameter_typed(&self) -> Result<f64, TeethFailure> {
        Ok(2.0 * hub_profile(&self.shaft.profile()?, &self.fit)?.outer)
    }

    /// The diameter over the tips of the hub's teeth, in millimetres.
    pub fn minor_diameter(&self) -> crate::Result<f64> {
        self.minor_diameter_typed()
            .map_err(|e| SolidError::teeth("teeth::straight::SerratedHub::minor_diameter", e))
    }

    /// [`Self::minor_diameter`] with its failure as a [`TeethFailure`], for the tooth module's own
    /// callers.
    pub(crate) fn minor_diameter_typed(&self) -> Result<f64, TeethFailure> {
        Ok(2.0 * hub_profile(&self.shaft.profile()?, &self.fit)?.inner)
    }
}

/// The hub that fits a serrated shaft: its flanks moved away from the shaft's by the side
/// clearance, at right angles to them, the bottoms of its spaces beyond the shaft's major circle by
/// the major clearance and its teeth short of the shaft's minor circle by the minor clearance.
///
/// Refused when the hub cannot be made or its rounds would come closer to the shaft than the
/// smallest clearance.
pub fn serration_hub_for(shaft: &Serration, fit: &Fit) -> crate::Result<SerratedHub> {
    serration_hub_for_typed(shaft, fit)
        .map_err(|e| SolidError::teeth("teeth::straight::serration_hub_for", e))
}

/// [`serration_hub_for`] with its failure as a [`TeethFailure`], for the tooth module's own
/// callers.
pub(crate) fn serration_hub_for_typed(
    shaft: &Serration,
    fit: &Fit,
) -> Result<SerratedHub, TeethFailure> {
    hub_profile(&shaft.profile()?, fit)?;
    Ok(SerratedHub {
        shaft: *shaft,
        fit: *fit,
    })
}

/// A serrated shaft and the hub that fits it as a pair on one axis, turning as one.
pub fn serration_pair(shaft: &Serration, fit: &Fit, tolerance: f64) -> crate::Result<Pair> {
    serration_pair_typed(shaft, fit, tolerance)
        .map_err(|e| SolidError::teeth("teeth::straight::serration_pair", e))
}

/// [`serration_pair`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
pub(crate) fn serration_pair_typed(
    shaft: &Serration,
    fit: &Fit,
    tolerance: f64,
) -> Result<Pair, TeethFailure> {
    let profile = shaft.profile()?;
    pair_of(&profile, &hub_profile(&profile, fit)?, tolerance)
}

/// How far, in radians, the hub of [`serration_pair`] turns on its shaft either way from centred
/// before a flank touches: found exactly from the lines and arcs of the two outlines.
pub fn serration_play(shaft: &Serration, fit: &Fit) -> crate::Result<f64> {
    serration_play_typed(shaft, fit)
        .map_err(|e| SolidError::teeth("teeth::straight::serration_play", e))
}

/// [`serration_play`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
pub(crate) fn serration_play_typed(shaft: &Serration, fit: &Fit) -> Result<f64, TeethFailure> {
    let profile = shaft.profile()?;
    play_of_profiles(&profile, &hub_profile(&profile, fit)?)
}
