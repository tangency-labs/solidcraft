//! Where a spline sits on a round face of a part, and the measures a caller that cuts it takes of
//! the face and of what it cuts: the frame at the face's free end (angle 0 from the world axes),
//! the face's distances from a point, a point moved out from the axis, the map into the frame,
//! how a line or a chord lies to the axis, the angle of a box's middle, and which way a tool lies
//! once it is turned (a copy turned about the axis, a quarter turn into the shaft).
//!
//! A round face is plain numbers ([`RoundFace`]): a point on its axis, the axis, the radius, and
//! where the face starts and ends along the axis. The frame is a [`Frame`] whose x is angle 0 and
//! whose normal (x cross y) is the axis as the world axes orient it.
//!
//! The measures are the kernel's own: the frame is [`Transform::orthonormal_frame`], the map into
//! it [`Transform::inverse`] of the frame's placement, a point's distance from a line
//! [`line_point_distance`], a box's reach along a direction [`Extent3d::span_along`].
//!
//! Not upstream: SolidCraft's shaft spline, a kernel primitive because it computes geometry any
//! client could use (docs/layers.md §1). No recursion. Inputs are hostile: a direction that has
//! no length, a face of no size, numbers that are not finite give a refusal or a plain answer,
//! never a panic.

use std::f64::consts::FRAC_PI_2;

use crate::eval::Frame;
use crate::geom::extent3d::Extent3d;
use crate::geom::geom_utility::p1::line_point_distance;
use crate::math::Vec2;
use crate::{Transform, Vec3};

/// The least length a direction has to be made a unit vector of.
const LEAST_LENGTH: f64 = 1e-9;

/// The least sine of the turn from one tool to its copy that tells the turn's two senses apart
/// by where the copy lies: a turn closer than this to none or to half a turn puts the copy nearly
/// where either sense would.
const TELLING_TURN: f64 = 1e-3;

/// A round face of a part as numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RoundFace {
    /// A point on its axis.
    pub axis_point: Vec3,
    /// The axis, a unit vector.
    pub axis: Vec3,
    /// Its radius.
    pub radius: f64,
    /// Where the face starts along the axis, measured from the axis point.
    pub start: f64,
    /// Where the face ends along the axis, measured from the axis point.
    pub end: f64,
}

/// Where a spline sits on a round face.
#[derive(Clone, Debug, PartialEq)]
pub struct SeatFrame {
    /// The plane square to the axis at the free end: its x is angle 0, its normal (x cross y) the
    /// axis as the world axes orient it.
    pub plane: Frame,
    /// 1.0 when the face runs on from the free end along the plane's normal, -1.0 when against it.
    pub inward: f64,
    /// The length of the face along its axis (`end - start`).
    pub room: f64,
    /// The pick, brought onto the face's cylinder along its radius.
    pub at: Vec3,
}

/// Why a round face gives no seat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeatRefusal {
    /// The axis has no direction.
    NoDirection,
    /// No plane square to the axis can be made.
    NoPlane,
    /// The face has no length or no radius.
    NoSize,
}

/// How one line or axis lies to another.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AxisGap {
    /// The length of the cross product of the two directions: 0 when they are parallel.
    pub lean: f64,
    /// The distance of the point from the axis line.
    pub off: f64,
}

/// A chord against an axis.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ChordGap {
    /// The length of the chord.
    pub length: f64,
    /// The length of the cross product of the chord with the axis.
    pub lean: f64,
}

/// A vector of a frame's numbers.
fn v(a: [f64; 3]) -> Vec3 {
    Vec3::from_array(a)
}

/// The unit vector along `d`, where `d` is longer than [`LEAST_LENGTH`] and finite.
fn normalized(d: Vec3) -> Option<Vec3> {
    let l = d.length();
    (l > LEAST_LENGTH && l.is_finite()).then(|| d / l)
}

/// Whether every coordinate of `p` is finite.
fn finite(p: Vec3) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.z.is_finite()
}

/// The normal of a frame: its x cross its y.
#[must_use]
pub fn normal(plane: &Frame) -> Vec3 {
    v(plane.x_axis).cross(v(plane.y_axis))
}

/// The frame through `origin` with x along `x` and y square to it on the side of `y`: the
/// kernel's orthonormal frame of the two ([`Transform::orthonormal_frame`]). None where a number
/// is not finite, or `y` has no length or no direction across `x`.
fn frame_at(origin: Vec3, x: Vec3, y: Vec3) -> Option<Frame> {
    if !(finite(origin) && finite(x) && finite(y) && x.cross(y).length() > LEAST_LENGTH) {
        return None;
    }
    let [x, y] = Transform::orthonormal_frame(x, y).ok()?;
    (finite(x) && finite(y)).then(|| Frame {
        origin: origin.to_array(),
        x_axis: x.to_array(),
        y_axis: y.to_array(),
    })
}

/// Angle 0 about an axis and the axis as the world axes orient it: the world axis the axis is
/// closest to decides (Z, then X, then Y on a tie), the axis is turned to point to that world
/// axis's positive side, and the world axis's reference direction (+X for Z, +Y for X, +Z for Y)
/// is projected onto the plane square to it. None where that projection has no direction.
#[must_use]
pub fn angle_zero(axis: Vec3) -> Option<(Vec3, Vec3)> {
    let (ax, ay, az) = (axis.x.abs(), axis.y.abs(), axis.z.abs());
    let (world, reference) = if az >= ax && az >= ay {
        (Vec3::Z, Vec3::X)
    } else if ax >= ay {
        (Vec3::X, Vec3::Y)
    } else {
        (Vec3::Y, Vec3::Z)
    };
    let axis = if axis.dot(world) < 0.0 {
        axis * -1.0
    } else {
        axis
    };
    let x = normalized(reference - axis * reference.dot(axis))?;
    Some((x, axis))
}

/// How far `p` is from the cylinder of the round face: the distance of `p` from the axis line,
/// less the radius, as an absolute value. Not the distance past the face's ends.
#[must_use]
pub fn off_cylinder(face: &RoundFace, p: Vec3) -> f64 {
    let k = face.axis;
    let d = p - face.axis_point;
    ((d - k * d.dot(k)).length() - face.radius).abs()
}

/// Whether `p` lies nearer along the axis to the face's `start` end than to its `end` (a tie is the
/// start's).
#[must_use]
pub fn nearer_start(face: &RoundFace, p: Vec3) -> bool {
    let along = (p - face.axis_point).dot(face.axis);
    (along - face.start).abs() <= (face.end - along).abs()
}

/// The seat of a spline on `face`: the free end is the end nearest along the axis to `start` (a
/// point on the face the teeth start from), or with none the end nearest `pick`. The face runs on
/// from the free end into the part; the plane at the free end is square to the axis with angle 0
/// from the world axes ([`angle_zero`]); the pick is brought onto the face's cylinder along its
/// radius (a pick on the axis stays where it is).
pub fn seat_frame(
    face: &RoundFace,
    pick: Vec3,
    start: Option<Vec3>,
) -> Result<SeatFrame, SeatRefusal> {
    let k = face.axis;
    // The teeth start at the end the start face lies at; the round face runs on from it.
    let at_start = nearer_start(face, start.unwrap_or(pick));
    let end_at = if at_start { face.start } else { face.end };
    let end = face.axis_point + k * end_at;
    let into = if at_start { k } else { k * -1.0 };
    let (x, normal_axis) = angle_zero(k).ok_or(SeatRefusal::NoDirection)?;
    let plane = frame_at(end, x, normal_axis.cross(x)).ok_or(SeatRefusal::NoPlane)?;
    let inward = if into.dot(normal(&plane)) > 0.0 {
        1.0
    } else {
        -1.0
    };
    let room = face.end - face.start;
    if !(room.is_finite() && room > 0.0 && face.radius.is_finite() && face.radius > 0.0) {
        return Err(SeatRefusal::NoSize);
    }
    let off = pick - face.axis_point;
    let across = off - k * off.dot(k);
    let at = if across.length() > 0.0 {
        pick + across * (face.radius / across.length() - 1.0)
    } else {
        pick
    };
    Ok(SeatFrame {
        plane,
        inward,
        room,
        at,
    })
}

/// The points a spline's start is looked for at on `face`: in the plane of the end of the face
/// nearest `pick`, along angle 0, the points half, a quarter and a tenth of the radius off the rim
/// (inside the rim of a shaft, outside the rim of a `bore`), then the point on the axis. None
/// where the axis has no angle 0.
#[must_use]
pub fn start_candidates(face: &RoundFace, pick: Vec3, bore: bool) -> Option<[Vec3; 4]> {
    let k = face.axis;
    let end = face.axis_point
        + k * if nearer_start(face, pick) {
            face.start
        } else {
            face.end
        };
    let (x, _) = angle_zero(k)?;
    let beside =
        |share: f64| end + x * (face.radius * if bore { 1.0 + share } else { 1.0 - share });
    Some([beside(0.5), beside(0.25), beside(0.1), end])
}

/// `at` moved `by` straight out from the axis, the line through the plane's origin along its
/// normal; `at` itself where it lies on the axis.
#[must_use]
pub fn moved_out(plane: &Frame, at: Vec3, by: f64) -> Vec3 {
    let off = at - v(plane.origin);
    let k = normal(plane);
    let across = off - k * off.dot(k);
    if across.length() > 0.0 {
        at + across * (by / across.length())
    } else {
        at
    }
}

/// The map that takes world coordinates into the plane's frame (x, y, normal; the origin to
/// zero), as the kernel's [`Transform`] matrix (rows the images of the world axes, then the
/// translation): the inverse of the frame's placement. None where the kernel proves no inverse
/// (a number not finite, axes that do not span space).
#[must_use]
pub fn into_frame(plane: &Frame) -> Option<[[f64; 4]; 4]> {
    let placed = Transform::from_axes(
        v(plane.x_axis),
        v(plane.y_axis),
        normal(plane),
        v(plane.origin),
    );
    placed.inverse().ok().map(|map| map.m)
}

/// How the line through `point` along `dir` lies to the axis through `axis_point` along `axis`
/// (unit vectors): the lean of the two directions, and the kernel's distance of `point` from the
/// axis line ([`line_point_distance`]).
#[must_use]
pub fn axis_gap(axis_point: Vec3, axis: Vec3, point: Vec3, dir: Vec3) -> AxisGap {
    AxisGap {
        lean: dir.cross(axis).length(),
        off: line_point_distance(axis_point, axis, point).0,
    }
}

/// The chord from `a` to `b` against the direction `axis` (a unit vector).
#[must_use]
pub fn chord_gap(axis: Vec3, a: Vec3, b: Vec3) -> ChordGap {
    let d = b - a;
    ChordGap {
        length: d.length(),
        lean: d.cross(axis).length(),
    }
}

/// The angle about the plane's normal, from its x towards its y, of the middle of the box
/// `min`..`max`.
#[must_use]
pub fn centre_angle(plane: &Frame, min: Vec3, max: Vec3) -> f64 {
    let centre = (min + max) * 0.5 - v(plane.origin);
    centre
        .dot(v(plane.y_axis))
        .atan2(centre.dot(v(plane.x_axis)))
}

/// The angle a tool made about the middle line at the angle `from` is turned by about the axis to
/// lie about the middle line at `to` (radians): `to - from`, or its opposite where the turns go
/// the other way (`back`, as [`lies_turned_back`] tells).
#[must_use]
pub fn copy_turn(from: f64, to: f64, back: bool) -> f64 {
    let by = to - from;
    if back {
        -by
    } else {
        by
    }
}

/// Whether a copy turned from the middle line at `from` to the one at `to` tells the two senses of
/// turning apart by where it lies: not where the turn is so near none or half a turn that either
/// sense puts the copy in nearly the same place (or the angles are not numbers).
#[must_use]
pub fn turn_tells_sense(from: f64, to: f64) -> bool {
    (to - from).sin().abs() > TELLING_TURN
}

/// Whether a copy turned by [`copy_turn`] from `from` to `to` (not back) lies about the mirror of
/// `to` instead (`from` turned the other way), so that the turns go the other way: the middle of
/// its box `min`..`max` ([`centre_angle`] about the plane's normal) is nearer, the short way round,
/// to that mirror than to `to`. Asked only where [`turn_tells_sense`].
#[must_use]
pub fn lies_turned_back(plane: &Frame, min: Vec3, max: Vec3, from: f64, to: f64) -> bool {
    let lies = centre_angle(plane, min, max);
    let mirror = from - (to - from);
    // Nearer the short way round is a larger cosine of the angle between.
    (lies - mirror).cos() > (lies - to).cos()
}

/// The quarter turn that takes a section on the plane into the shaft (radians): a quarter turn
/// towards the plane's normal times `inward` (the kernel turns a section towards its plane's
/// normal), the other way where a tool turned that way lies outside (`back`, as [`lies_inward`]
/// tells).
#[must_use]
pub fn quarter_turn_in(inward: f64, back: bool) -> f64 {
    let turn = FRAC_PI_2 * inward;
    if back {
        -turn
    } else {
        turn
    }
}

/// Whether the box `min`..`max` of a turned tool lies past the plane into the shaft, along its
/// normal times `inward`: it reaches at least as far that way as it reaches back out
/// ([`Extent3d::span_along`] from the plane's origin). A box of numbers that are not numbers is
/// taken to lie inward, so nothing is turned again for it.
#[must_use]
pub fn lies_inward(plane: &Frame, inward: f64, min: Vec3, max: Vec3) -> bool {
    let into = normal(plane) * inward;
    let bounds = Extent3d::from_bounds(min.x, min.y, min.z, max.x, max.y, max.z);
    let [low, high] = bounds.span_along(v(plane.origin), into);
    !(high < -low)
}

/// The distance between two points of an outline, the kernel's ([`Vec2::distance`]).
#[must_use]
pub fn points_apart(a: [f64; 2], b: [f64; 2]) -> f64 {
    Vec2::new(a[0], a[1]).distance(Vec2::new(b[0], b[1]))
}
