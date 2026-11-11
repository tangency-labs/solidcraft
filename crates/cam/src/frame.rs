//! Frames: a right-handed orthonormal set of axes at an origin, for tilted work planes, tool
//! frames and rotary axes. Points and directions go between a frame and the coordinates it is
//! given in.

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::{Result, invalid};

/// Shortest axis accepted as a direction.
const MIN_LEN: f64 = 1e-12;

/// The unit vector along `v`, refused when `v` is not a usable direction.
pub fn unit(v: Vec3, what: &str) -> Result<Vec3> {
    if !v.is_finite() || !(v.len() > MIN_LEN) {
        return invalid(format!("{what} must be a finite, non-zero direction"));
    }
    v.normalized().map_or_else(|| invalid(format!("{what} must be a finite, non-zero direction")), Ok)
}

/// `v` turned by `angle` (radians, right-handed) about the unit direction `axis` through the origin.
pub fn rotate(v: Vec3, axis: Vec3, angle: f64) -> Vec3 {
    let (s, c) = angle.sin_cos();
    v * c + axis.cross(v) * s + axis * (axis.dot(v) * (1.0 - c))
}

/// `p` turned by `angle` about the line through `point` along the unit direction `axis`.
pub fn rotate_about(p: Vec3, point: Vec3, axis: Vec3, angle: f64) -> Vec3 {
    point + rotate(p - point, axis, angle)
}

/// The angle (radians) in `-π..=π` that turns `from` into `to` about the unit `axis`, both seen
/// along the axis; `None` when either lies along the axis, where any angle does.
pub fn angle_about(from: Vec3, to: Vec3, axis: Vec3) -> Option<f64> {
    let u = from - axis * axis.dot(from);
    let v = to - axis * axis.dot(to);
    if u.len() <= 1e-9 || v.len() <= 1e-9 {
        return None;
    }
    Some(axis.dot(u.cross(v)).atan2(u.dot(v)))
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub origin: Vec3,
    pub x: Vec3,
    pub y: Vec3,
    pub z: Vec3,
}

impl Default for Frame {
    fn default() -> Frame {
        Frame::IDENTITY
    }
}

impl Frame {
    pub const IDENTITY: Frame =
        Frame { origin: Vec3::new(0.0, 0.0, 0.0), x: Vec3::new(1.0, 0.0, 0.0), y: Vec3::new(0.0, 1.0, 0.0), z: Vec3::new(0.0, 0.0, 1.0) };

    /// The frame at `origin` whose Z is `z` and whose X is `x_hint` made square to it.
    pub fn new(origin: Vec3, z: Vec3, x_hint: Vec3) -> Result<Frame> {
        if !origin.is_finite() {
            return invalid("a frame's origin must be finite");
        }
        let z = unit(z, "a frame's Z axis")?;
        let square = x_hint - z * z.dot(x_hint);
        // What is left of a hint nearly along Z is mostly rounding: refuse it.
        if !(square.len() > 1e-6 * x_hint.len()) {
            return invalid("a frame's X axis must not lie along its Z axis");
        }
        let x = unit(square, "a frame's X axis (it must not lie along Z)")?;
        // Once more, for what rounding left in the first pass.
        let x = unit(x - z * z.dot(x), "a frame's X axis (it must not lie along Z)")?;
        let frame = Frame { origin, x, y: z.cross(x), z };
        frame.validate()?;
        Ok(frame)
    }

    /// The frame at `origin` whose Z is `z`, with X the world X made square to it (the world Y
    /// when Z is close to the world X). The same Z always gives the same frame.
    pub fn from_z(origin: Vec3, z: Vec3) -> Result<Frame> {
        let zu = unit(z, "a frame's Z axis")?;
        let hint = if zu.x.abs() < 0.9 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) };
        Frame::new(origin, zu, hint)
    }

    /// Whether the axes are unit length, square to each other and right-handed.
    pub fn is_orthonormal(&self) -> bool {
        let ok = |v: Vec3| v.is_finite() && (v.len() - 1.0).abs() <= 1e-9;
        ok(self.x)
            && ok(self.y)
            && ok(self.z)
            && self.x.dot(self.y).abs() <= 1e-9
            && self.y.dot(self.z).abs() <= 1e-9
            && self.z.dot(self.x).abs() <= 1e-9
            && self.x.cross(self.y).dist(self.z) <= 1e-9
            && self.origin.is_finite()
    }

    pub fn validate(&self) -> Result<()> {
        if self.is_orthonormal() { Ok(()) } else { invalid("a frame's axes must be unit length, square to each other and right-handed") }
    }

    /// A point given in the outer coordinates, in this frame's.
    pub fn to_local(&self, p: Vec3) -> Vec3 {
        let d = p - self.origin;
        Vec3::new(d.dot(self.x), d.dot(self.y), d.dot(self.z))
    }

    /// A point given in this frame, in the outer coordinates.
    pub fn to_world(&self, p: Vec3) -> Vec3 {
        self.origin + self.x * p.x + self.y * p.y + self.z * p.z
    }

    pub fn dir_to_local(&self, d: Vec3) -> Vec3 {
        Vec3::new(d.dot(self.x), d.dot(self.y), d.dot(self.z))
    }

    pub fn dir_to_world(&self, d: Vec3) -> Vec3 {
        self.x * d.x + self.y * d.y + self.z * d.z
    }

    /// The frame `inner` (given in this frame's coordinates) in the outer coordinates.
    pub fn compose(&self, inner: &Frame) -> Frame {
        Frame { origin: self.to_world(inner.origin), x: self.dir_to_world(inner.x), y: self.dir_to_world(inner.y), z: self.dir_to_world(inner.z) }
    }

    /// The frame that undoes this one: this frame's outer coordinates, given in this frame.
    pub fn inverse(&self) -> Frame {
        let t = Frame::IDENTITY;
        Frame { origin: self.to_local(t.origin), x: self.dir_to_local(t.x), y: self.dir_to_local(t.y), z: self.dir_to_local(t.z) }
    }
}
