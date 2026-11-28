//! Master splines: one tooth of a splined shaft and its hub made different from the others, so that
//! the hub goes on the shaft one way only, as a removable steering wheel must.
//!
//! A master spline is made in one of two forms ([`Form`]):
//!
//! - **A tooth left out** ([`Form::ToothLeftOut`]): one tooth of the shaft is not there, and the
//!   hub's space that would hold it is left uncut, so the hub keeps one wide tooth that sits in the
//!   shaft's gap.
//! - **A wide tooth** ([`Form::WideTooth`]): one space of the shaft is left uncut, which joins two
//!   of its teeth into one wide tooth, and the hub's tooth that would sit in that space is left
//!   out.
//!
//! Both are made from the outlines of an ordinary shaft and hub, without new curves: one period of
//! an outline (a tooth and a space, from a point on a circle about the axis to the same point a
//! pitch on) is replaced by an arc of that circle ([`replaced_period`]). For a shaft the smallest
//! circle is its root circle, so an arc there takes a tooth away; the largest is its tip circle, so
//! an arc there fills a space. For a hub's opening the smallest circle is the circle of the tips of
//! its teeth, so an arc there fills a space, and the largest the circle of the bottoms of its
//! spaces, so an arc there takes a tooth away. Every other piece is the outline's own piece, kept
//! exact and turned to its place, but for the outline's own arcs on that circle that the new arc
//! runs on into: they and the arc are one curve, so they are made one arc (one piece of a
//! profile, one face of a body: the bottom of the space a tooth is left out of is one face from
//! flank to flank). Nothing comes closer to the other part than before, so the
//! clearances of the fit are kept; turned on by a pitch, the hub meets the shaft, so it goes on in
//! one position only.
//!
//! [`placement`] finds the tooth (or the space) nearest to an angle and the turn of the whole
//! spline that puts the middle of the master spline exactly at that angle; [`master_spline`] does
//! it all for a pair of shaft and hub.
//!
//! The arc that replaces a period runs from a joint of the pieces on the circle to the same joint a
//! pitch on, so its middle need not be the middle of the tooth left out or of the space filled: a
//! tooth's own pieces need not sit symmetrically about that joint. Where the master spline is, is
//! [`MasterSpline::middle`].
//!
//! An outline with a master spline does not repeat around its axis, so it is an outline of one
//! tooth that goes all the way round, with a pair cycle of a whole turn. Angles are radians, except
//! the angle a person gives [`placement`] and [`master_spline`], which is degrees.

use crate::math::det;
use std::f64::consts::TAU;

use crate::error::teeth::{TeethFailure, Why};
use crate::teeth::mesh::Material;
use crate::teeth::outline::{rotated, Outline, Piece, Point, Role};
use crate::teeth::pair::{Motion, Pair};
use crate::teeth::MAX_TEETH;
use crate::SolidError;

/// Which part gives up a tooth for the master spline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    /// A tooth is left out of the shaft and the hub's space for it is left uncut: the hub has one
    /// wide tooth.
    ToothLeftOut,
    /// A space of the shaft is left uncut, which joins two of its teeth into one wide tooth, and a
    /// tooth is left out of the hub.
    WideTooth,
}

/// Which circle of an outline the arc that replaces a period lies on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Circle {
    /// The outline's smallest circle about the axis: a shaft's root circle, a hub's circle of tips.
    Smallest,
    /// The outline's largest circle about the axis: a shaft's tip circle, a hub's circle of roots.
    Largest,
}

impl Form {
    /// The circle on which the arcs of this form lie, on the shaft and in the hub alike.
    pub fn circle(self) -> Circle {
        match self {
            Form::ToothLeftOut => Circle::Smallest,
            Form::WideTooth => Circle::Largest,
        }
    }
}

/// An arc about the axis: its radius and the angles (radians, counter-clockwise from the x axis) it
/// starts and ends at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    /// The radius of the arc, in millimetres.
    pub radius: f64,
    /// The angle of its first point.
    pub start: f64,
    /// The angle of its last point, a pitch on from the first.
    pub end: f64,
}

/// An outline with one period replaced by an arc, and where that arc lies.
#[derive(Clone, Debug, PartialEq)]
pub struct Replaced {
    /// The outline: one tooth that goes all the way round.
    pub outline: Outline,
    /// The arc that takes the place of the period: from the joint on the circle where the period
    /// starts to the same joint a pitch on.
    pub arc: Span,
}

/// Where a master spline goes on a spline of a count of teeth, for an angle a person gives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The tooth left out, or for a wide tooth the tooth before the space that is left uncut,
    /// counted counter-clockwise from the tooth on the x axis (0).
    pub tooth: u32,
    /// How many pitches the middle of the master spline is from the tooth on the x axis before the
    /// turn: a whole count for a tooth left out, a whole count and a half for a wide tooth (signed
    /// as the angle was given).
    pub middle: f64,
    /// The turn of the whole spline, shaft and hub together, in radians, that puts the middle of
    /// the master spline exactly at the angle given. At most half a pitch either way: half a pitch
    /// back when the angle is just between two.
    pub turn: f64,
}

/// A shaft and its hub with their master spline.
#[derive(Clone, Debug, PartialEq)]
pub struct MasterSpline {
    /// The pair: the same axes, motion and start of the hub as the pair without the master spline,
    /// both outlines turned by the placement's turn, and a cycle of a whole turn.
    pub pair: Pair,
    /// Where the master spline is.
    pub placement: Placement,
    /// The angle of the middle of the master spline after the turn, in radians counter-clockwise
    /// from the x axis: the middle of the tooth left out, or of the space filled, on the shaft. It
    /// is the angle given.
    pub middle: f64,
    /// The arc of the shaft that replaces a period, after the turn. It runs from joint to joint on
    /// the circle, a pitch long, and covers the tooth left out (or the space filled); its middle
    /// can be off [`MasterSpline::middle`] by a part of a pitch.
    pub shaft_arc: Span,
    /// The arc of the hub that replaces a period, in the hub's own coordinates, after the turn: add
    /// the pair's `second_start` to lay it over the shaft. It runs from joint to joint as the
    /// shaft's does.
    pub hub_arc: Span,
}

/// The largest angle accepted by [`placement`], either way, in degrees.
pub const MOST_ANGLE: f64 = 360.0;

/// A piece of an outline turned about the axis by `angle`.
pub(super) fn turned_piece(piece: &Piece, angle: f64) -> Piece {
    match piece {
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
        Piece::Line { from, to, role } => Piece::Line {
            from: rotated(*from, angle),
            to: rotated(*to, angle),
            role: *role,
        },
        Piece::Curve { points, role } => Piece::Curve {
            points: points.iter().map(|&p| rotated(p, angle)).collect(),
            role: *role,
        },
        Piece::Spline {
            degree,
            knots,
            control,
            role,
        } => Piece::Spline {
            degree: *degree,
            knots: knots.clone(),
            control: control.iter().map(|&p| rotated(p, angle)).collect(),
            role: *role,
        },
    }
}

/// The angle of a point about the axis.
fn angle_of(p: Point) -> f64 {
    det::atan2(p[1], p[0])
}

/// The outline turned about its axis by `angle` (radians, counter-clockwise): the same pieces at
/// their new places, with the same count of teeth and the same tolerance.
pub fn turned(outline: &Outline, angle: f64) -> crate::Result<Outline> {
    turned_typed(outline, angle).map_err(|e| SolidError::teeth("teeth::master_spline::turned", e))
}

/// [`turned`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
pub(crate) fn turned_typed(outline: &Outline, angle: f64) -> Result<Outline, TeethFailure> {
    if !angle.is_finite() {
        return Err(TeethFailure::Invalid(Why::AngleNotANumber));
    }
    let tooth = outline
        .tooth()
        .iter()
        .map(|p| turned_piece(p, angle))
        .collect();
    Outline::new_typed(outline.teeth(), tooth, outline.tolerance())
}

/// One period of an outline that starts and ends on one of its circles about the axis.
struct Period {
    /// Its pieces, counter-clockwise.
    pieces: Vec<Piece>,
    /// The angle of its first point.
    start: f64,
    /// The distance of its first point from the axis.
    radius: f64,
    /// The angle from one period to the next.
    pitch: f64,
    /// How many periods go round.
    teeth: u32,
}

impl Period {
    /// The period of an outline taken from a joint of its pieces that lies on the circle asked for:
    /// the pieces before that joint come after the others, turned on by one pitch. Refused if no
    /// joint lies on that circle.
    fn of(outline: &Outline, circle: Circle) -> Result<Period, TeethFailure> {
        let tooth = outline.tooth();
        let on = match circle {
            Circle::Smallest => outline.radii().0,
            Circle::Largest => outline.radii().1,
        };
        let near = 1e-6 * (1.0 + on);
        let joint = tooth
            .iter()
            .enumerate()
            .filter_map(|(i, p)| p.start().map(|q| (i, q, det::hypot(q[0], q[1]))))
            .filter(|(_, _, r)| (r - on).abs() <= near)
            .min_by(|a, b| (a.2 - on).abs().total_cmp(&(b.2 - on).abs()));
        let Some((at, first, radius)) = joint else {
            return Err(TeethFailure::Invalid(match circle {
                Circle::Smallest => Why::NoCornerOnSmallest,
                Circle::Largest => Why::NoCornerOnLargest,
            }));
        };
        let pitch = outline.pitch_angle();
        let pieces = tooth
            .iter()
            .skip(at)
            .cloned()
            .chain(tooth.iter().take(at).map(|p| turned_piece(p, pitch)))
            .collect();
        Ok(Period {
            pieces,
            start: angle_of(first),
            radius,
            pitch,
            teeth: outline.teeth(),
        })
    }

    /// The number of the period that holds the angle `angle`, counted from this one.
    fn holding(&self, angle: f64) -> u32 {
        let k = ((angle - self.start).rem_euclid(TAU) / self.pitch).floor();
        let k = if k.is_finite() && k > 0.0 { k } else { 0.0 };
        // In range and whole, so the cast loses nothing
        (k as u32).min(self.teeth.saturating_sub(1))
    }

    /// Where the arc that takes the place of period `k` starts, and where it ends.
    fn arc_of(&self, k: u32) -> Span {
        let start = self.start + self.pitch * f64::from(k);
        Span {
            radius: self.radius,
            start,
            end: start + self.pitch,
        }
    }
}

/// The outline with the period that holds the angle `at` (radians) replaced by an arc about the
/// axis on the circle asked for, as one tooth that goes all the way round. A period runs from a
/// joint of the outline's pieces that lies on that circle to the same joint a pitch on. Only whole
/// pieces of the outline are kept or dropped, turned to their places, and the outline keeps its
/// tolerance. An arc of the outline about the axis on that circle that the new arc runs on into
/// (before it, after it, or both) is made one arc with it, on the outline's own centre and
/// radius: the stretch is one curve, so it is one piece.
///
/// The arc takes the role of the outline's own arc about the axis on that circle. Where there is
/// none, it takes the role that circle has on a part whose material lies as `material` says: on a
/// shaft (material inside) the smallest circle is the root and the largest the tip; in a hub's
/// opening (material outside) it is the other way round.
///
/// Refused when the outline has fewer than two teeth (nothing would be left of it), when `at` is
/// not a number ([`Why::AngleNotANumber`]), and when no joint of its pieces lies on the circle
/// ([`Why::NoCornerOnSmallest`], [`Why::NoCornerOnLargest`]).
pub fn replaced_period(
    outline: &Outline,
    material: Material,
    circle: Circle,
    at: f64,
) -> crate::Result<Replaced> {
    replaced_period_typed(outline, material, circle, at)
        .map_err(|e| SolidError::teeth("teeth::master_spline::replaced_period", e))
}

/// [`replaced_period`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
pub(crate) fn replaced_period_typed(
    outline: &Outline,
    material: Material,
    circle: Circle,
    at: f64,
) -> Result<Replaced, TeethFailure> {
    if outline.teeth() < 2 {
        let (value, up_to) = (f64::from(outline.teeth()), f64::from(MAX_TEETH));
        return Err(TeethFailure::OutOfRange {
            what: "teeth",
            value,
            above: 1.0,
            up_to,
        });
    }
    if !at.is_finite() {
        return Err(TeethFailure::Invalid(Why::AngleNotANumber));
    }
    let period = Period::of(outline, circle)?;
    // The arc takes the role of the arc about the axis on that circle, where there is one.
    let role = period
        .pieces
        .iter()
        .find_map(|p| match p {
            Piece::Arc {
                centre,
                radius,
                role,
                ..
            } if det::hypot(centre[0], centre[1]) < 1e-9
                && (radius - period.radius).abs() <= 1e-6 * (1.0 + radius) =>
            {
                Some(*role)
            }
            _ => None,
        })
        .unwrap_or(match (material, circle) {
            (Material::Inside, Circle::Smallest) | (Material::Outside, Circle::Largest) => {
                Role::Root
            }
            (Material::Inside, Circle::Largest) | (Material::Outside, Circle::Smallest) => {
                Role::Tip
            }
        });
    let leave_out = period.holding(at);
    let arc = period.arc_of(leave_out);
    let mut pieces = Vec::new();
    let mut placed = 0;
    for k in 0..period.teeth {
        if k == leave_out {
            placed = pieces.len();
            pieces.push(Piece::Arc {
                centre: [0.0, 0.0],
                radius: period.radius,
                start: arc.start,
                sweep: period.pitch,
                role,
            });
        } else {
            let turn = period.pitch * f64::from(k);
            pieces.extend(period.pieces.iter().map(|p| turned_piece(p, turn)));
        }
    }
    Ok(Replaced {
        outline: Outline::new_typed(1, run_on(pieces, placed), outline.tolerance())?,
        arc,
    })
}

/// How far apart, as a share of the radius, the radii of two arcs about the axis may be for the
/// arcs to lie on one circle: the rounding of a point's distance from the axis (its two
/// coordinates, each a product with a sine or a cosine, and their root), sixteen roundings wide.
const ON_ONE_CIRCLE: f64 = 16.0 * f64::EPSILON;

/// The pieces with the arc at `at` and the arcs it runs on into made one arc. A neighbour of the
/// arc, going round (the piece before it, the piece after it, or both), that is an arc about the
/// axis of the same radius ([`ON_ONE_CIRCLE`]), turning the same way, with the same role, lies on
/// the arc's own circle: the two are one curve, and as two pieces a body would have two faces
/// where it has one surface. The joined arc takes the place of the arc at `at` and keeps the
/// neighbour's own centre and radius (the outline's piece, kept exact), from the start of the
/// arc before through all the sweeps. Where no neighbour is such an arc the pieces are given
/// back as they are.
fn run_on(pieces: Vec<Piece>, at: usize) -> Vec<Piece> {
    let n = pieces.len();
    let Some(&Piece::Arc {
        radius,
        start,
        sweep,
        role,
        ..
    }) = pieces.get(at)
    else {
        return pieces;
    };
    if n < 3 {
        return pieces;
    }
    // The start and the sweep of a neighbour on the arc's circle, with its own centre and radius.
    let on_circle = |i: usize| match pieces.get(i) {
        Some(&Piece::Arc {
            centre,
            radius: r,
            start: s,
            sweep: w,
            role: other,
        }) if other == role
            && centre[0] == 0.0
            && centre[1] == 0.0
            && (r - radius).abs() <= ON_ONE_CIRCLE * radius
            && w > 0.0 =>
        {
            Some((centre, r, s, w))
        }
        _ => None,
    };
    // The pieces go round: the one before the first is the last, the one after the last the
    // first.
    let last = n.saturating_sub(1);
    let before = at.checked_sub(1).unwrap_or(last);
    let after = if at >= last { 0 } else { at.saturating_add(1) };
    let (b, a) = (on_circle(before), on_circle(after));
    let (centre, r, from) = match (b, a) {
        (Some((centre, r, s, _)), _) => (centre, r, s),
        (None, Some((centre, r, _, _))) => (centre, r, start),
        (None, None) => return pieces,
    };
    let total = b.map_or(0.0, |x| x.3) + sweep + a.map_or(0.0, |x| x.3);
    if !(total < TAU) {
        return pieces;
    }
    let joined = Piece::Arc {
        centre,
        radius: r,
        start: from,
        sweep: total,
        role,
    };
    pieces
        .into_iter()
        .enumerate()
        .filter_map(|(i, p)| {
            if i == at {
                Some(joined.clone())
            } else if (b.is_some() && i == before) || (a.is_some() && i == after) {
                None
            } else {
                Some(p)
            }
        })
        .collect()
}

/// Where a master spline goes on a spline of `teeth` teeth so that its middle is at `angle` degrees
/// (counter-clockwise from the x axis, from -360 to 360): the tooth nearest to that angle (for a
/// wide tooth, the space nearest to it), and the turn of the whole spline, at most half a pitch,
/// that puts the middle exactly there. An angle just between two takes the one further on.
pub fn placement(teeth: u32, form: Form, angle: f64) -> crate::Result<Placement> {
    placement_typed(teeth, form, angle)
        .map_err(|e| SolidError::teeth("teeth::master_spline::placement", e))
}

/// [`placement`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
pub(crate) fn placement_typed(
    teeth: u32,
    form: Form,
    angle: f64,
) -> Result<Placement, TeethFailure> {
    if !(2..=MAX_TEETH).contains(&teeth) {
        let (value, up_to) = (f64::from(teeth), f64::from(MAX_TEETH));
        return Err(TeethFailure::OutOfRange {
            what: "teeth",
            value,
            above: 1.0,
            up_to,
        });
    }
    if !(angle.is_finite() && angle.abs() <= MOST_ANGLE) {
        return Err(TeethFailure::Invalid(Why::MasterAngleRange));
    }
    let z = f64::from(teeth);
    // The middle of a wide tooth is half a pitch on from the tooth before it.
    let half = if form == Form::WideTooth { 0.5 } else { 0.0 };
    // The nearest tooth (or space), as a whole count of pitches from -z to z.
    let near = (angle / 360.0 * z - half + 0.5).floor();
    // Whole and below z, so the cast loses nothing
    let tooth = (near.rem_euclid(z) as u32).min(teeth - 1);
    let middle = near + half;
    Ok(Placement {
        tooth,
        middle,
        turn: (angle - middle * 360.0 / z).to_radians(),
    })
}

/// The pair of a shaft and its hub with a master spline of `form` at `tooth` (counted
/// counter-clockwise from the tooth on the x axis; for a wide tooth, the tooth before the space
/// that is filled): the shaft's tooth left out and the hub's space that holds it filled, or the
/// shaft's space after that tooth filled and the hub's tooth that sits in it left out. The pair
/// keeps its axes, its motion and the hub's start; its cycle is a whole turn. Nothing is turned:
/// see [`master_spline`] for that.
///
/// The arcs it returns, on the shaft and in the hub's own coordinates, are where each part differs
/// from its ordinary teeth. Refused as [`replaced_period`] refuses, when `tooth` is not one of the
/// shaft's teeth, and when the pair is not a shaft and its hub ([`Why::NotAShaftAndHub`]): the
/// same count of teeth, the first part's material inside its outline and the second's outside, one
/// axis, and turning as one.
pub fn master_pair(pair: &Pair, form: Form, tooth: u32) -> crate::Result<(Pair, Span, Span)> {
    master_pair_typed(pair, form, tooth)
        .map_err(|e| SolidError::teeth("teeth::master_spline::master_pair", e))
}

/// [`master_pair`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
pub(crate) fn master_pair_typed(
    pair: &Pair,
    form: Form,
    tooth: u32,
) -> Result<(Pair, Span, Span), TeethFailure> {
    let teeth = pair.first.outline.teeth();
    let one_axis = pair.first.axis == pair.second.axis;
    let as_one = matches!(pair.motion, Motion::Turning { ratio } if ratio == 1.0);
    let shaft_and_hub =
        pair.first.material == Material::Inside && pair.second.material == Material::Outside;
    if !(pair.second.outline.teeth() == teeth && shaft_and_hub && one_axis && as_one) {
        return Err(TeethFailure::Invalid(Why::NotAShaftAndHub));
    }
    if tooth >= teeth {
        let (value, up_to) = (f64::from(tooth), f64::from(teeth.saturating_sub(1)));
        return Err(TeethFailure::OutOfRange {
            what: "tooth",
            value,
            above: -1.0,
            up_to,
        });
    }
    if !pair.second_start.is_finite() {
        return Err(TeethFailure::Invalid(Why::HubStartNotANumber));
    }
    let circle = form.circle();
    // The middle of the master spline, on the shaft and in the hub's own coordinates: the tooth
    // left out, or the space that is filled, half a pitch on.
    let half = if form == Form::WideTooth { 0.5 } else { 0.0 };
    let at = pair.first.outline.pitch_angle() * (f64::from(tooth) + half);
    let shaft = replaced_period_typed(&pair.first.outline, Material::Inside, circle, at)?;
    let hub = replaced_period_typed(
        &pair.second.outline,
        Material::Outside,
        circle,
        at - pair.second_start,
    )?;
    let mut keyed = pair.clone();
    keyed.first.outline = shaft.outline;
    keyed.second.outline = hub.outline;
    keyed.cycle = TAU;
    Ok((keyed, shaft.arc, hub.arc))
}

/// A shaft and its hub with a master spline of `form` whose middle sits exactly at `angle` degrees
/// (counter-clockwise from the x axis, from -360 to 360): the nearest tooth or space is taken
/// ([`placement`]) and the whole spline, shaft and hub together, is turned by at most half a pitch
/// to put it there.
///
/// Refused as [`placement`] and [`master_pair`] refuse.
pub fn master_spline(pair: &Pair, form: Form, angle: f64) -> crate::Result<MasterSpline> {
    master_spline_typed(pair, form, angle)
        .map_err(|e| SolidError::teeth("teeth::master_spline::master_spline", e))
}

/// [`master_spline`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
pub(crate) fn master_spline_typed(
    pair: &Pair,
    form: Form,
    angle: f64,
) -> Result<MasterSpline, TeethFailure> {
    let placement = placement_typed(pair.first.outline.teeth(), form, angle)?;
    let (mut keyed, shaft_arc, hub_arc) = master_pair_typed(pair, form, placement.tooth)?;
    let turn = placement.turn;
    keyed.first.outline = turned_typed(&keyed.first.outline, turn)?;
    keyed.second.outline = turned_typed(&keyed.second.outline, turn)?;
    let moved = |s: Span| Span {
        radius: s.radius,
        start: s.start + turn,
        end: s.end + turn,
    };
    let middle = placement.middle * pair.first.outline.pitch_angle() + turn;
    Ok(MasterSpline {
        pair: keyed,
        placement,
        middle,
        shaft_arc: moved(shaft_arc),
        hub_arc: moved(hub_arc),
    })
}
