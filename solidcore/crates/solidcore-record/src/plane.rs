//! A sketch plane as data: an origin and two in-plane axes. Making one from directions and
//! mapping points to and from it are the kernel's.
use serde::{Deserialize, Serialize};

use crate::vec::Vec3;

/// A sketch plane: an origin and two orthonormal in-plane axes. The normal is `x × y`.
///
/// World point of sketch point `(u, v)` = `origin + u·x + v·y`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plane {
    /// The sketch origin.
    pub origin: Vec3,
    /// Sketch x, a unit vector.
    #[serde(rename = "x_dir")]
    pub x: Vec3,
    /// Sketch y, a unit vector perpendicular to `x`.
    #[serde(rename = "y_dir")]
    pub y: Vec3,
}

impl Plane {
    /// Top: sketch x = +X, y = +Y, normal +Z.
    pub const XY: Plane = Plane {
        origin: Vec3::ZERO,
        x: Vec3::X,
        y: Vec3::Y,
    };
    /// Front: sketch x = +X, y = −Z, normal +Y.
    pub const XZ: Plane = Plane {
        origin: Vec3::ZERO,
        x: Vec3::X,
        y: Vec3::new(0.0, 0.0, -1.0),
    };
    /// Right: sketch x = +Y, y = +Z, normal +X.
    pub const YZ: Plane = Plane {
        origin: Vec3::ZERO,
        x: Vec3::Y,
        y: Vec3::Z,
    };

    /// Named origin plane: `XY`, `XZ`, `YZ` (case-insensitive).
    pub fn named(name: &str) -> Option<Plane> {
        match name.to_ascii_uppercase().as_str() {
            "XY" | "TOP" => Some(Plane::XY),
            "XZ" | "FRONT" => Some(Plane::XZ),
            "YZ" | "RIGHT" => Some(Plane::YZ),
            _ => None,
        }
    }
}
