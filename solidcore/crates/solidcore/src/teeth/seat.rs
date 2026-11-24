//! Where a spline sits on a round face of a part, and the measures a caller that cuts it takes of
//! the face and of what it cuts: the frame at the face's free end (angle 0 from the world axes),
//! the face's distances from a point, a point moved out from the axis, the matrix into the frame,
//! how a line or a chord lies to the axis, the angle of a box's middle and a box's reach along a
//! direction.
//!
//! A round face is plain numbers ([`RoundFace`]): a point on its axis, the axis, the radius, and
//! where the face starts and ends along the axis. The frame is a [`Frame`] whose x is angle 0 and
//! whose normal (x cross y) is the axis as the world axes orient it.
//!
//! Not upstream: SolidCraft's shaft spline, a kernel primitive because it computes geometry any
//! client could use (docs/layers.md §1). No recursion. Inputs are hostile: a direction that has
//! no length, a face of no size, numbers that are not finite give a refusal or a plain answer,
//! never a panic.

use std::f64::consts::{PI, TAU};

use crate::eval::Frame;
use crate::Vec3;

/// The least length a direction has to be made a unit vector of.
const LEAST_LENGTH: f64 = 1e-9;

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

/// The frame through `origin` with the axes `x` and `y` made a right-handed orthonormal pair: x
/// along `x`, the normal along `x` cross `y`, y the normal cross x. None where an axis has no
/// direction or the origin is not finite.
#[must_use]
pub fn frame_through(origin: Vec3, x: Vec3, y: Vec3) -> Option<Frame> {
    let x = normalized(x)?;
    let n = normalized(x.cross(y))?;
    let y = normalized(n.cross(x))?;
    finite(origin).then(|| Frame {
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
    let plane = frame_through(end, x, normal_axis.cross(x)).ok_or(SeatRefusal::NoPlane)?;
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

/// The matrix that takes world coordinates into the plane's frame (x, y, normal; the origin to
/// zero), column by column: the images of the world axes, then of the origin.
#[must_use]
pub fn into_frame(plane: &Frame) -> [[f64; 4]; 4] {
    let (x, y, n, o) = (
        v(plane.x_axis),
        v(plane.y_axis),
        normal(plane),
        v(plane.origin),
    );
    [
        [x.x, y.x, n.x, 0.0],
        [x.y, y.y, n.y, 0.0],
        [x.z, y.z, n.z, 0.0],
        [-o.dot(x), -o.dot(y), -o.dot(n), 1.0],
    ]
}

/// How the line through `point` along `dir` lies to the axis through `axis_point` along `axis`
/// (unit vectors).
#[must_use]
pub fn axis_gap(axis_point: Vec3, axis: Vec3, point: Vec3, dir: Vec3) -> AxisGap {
    let off = point - axis_point;
    AxisGap {
        lean: dir.cross(axis).length(),
        off: (off - axis * off.dot(axis)).length(),
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

/// How far the angle `a` is from the angle `b`, the short way round (0 to pi).
#[must_use]
pub fn angles_apart(a: f64, b: f64) -> f64 {
    ((a - b + PI).rem_euclid(TAU) - PI).abs()
}

/// The least and the most the box `min`..`max` reaches along `dir` from `origin`, over its eight
/// corners.
#[must_use]
pub fn reach_along(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> (f64, f64) {
    let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
    for corner in 0..8 {
        let q = Vec3::new(
            if corner & 1 == 0 { min.x } else { max.x },
            if corner & 2 == 0 { min.y } else { max.y },
            if corner & 4 == 0 { min.z } else { max.z },
        );
        let h = (q - origin).dot(dir);
        low = low.min(h);
        high = high.max(h);
    }
    (low, high)
}

/// The distance between two points of an outline.
#[must_use]
pub fn points_apart(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
