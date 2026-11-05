//! Quantities and geometry with their units in their types. Lengths are millimetres, angles
//! radians. In JSON a quantity is its number, a point or direction its `[x, y, z]`.

use std::fmt;
use std::ops::{Add, Div, Mul, Neg, Sub};

use serde::{Deserialize, Serialize};

/// A length in millimetres.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Mm(pub f64);

/// An angle in radians.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Radians(pub f64);

/// Arithmetic and display for a quantity newtype over `f64`.
macro_rules! quantity_ops {
    ($($name:ident: $unit:literal;)*) => {$(
        impl Add for $name {
            type Output = $name;
            fn add(self, o: $name) -> $name {
                $name(self.0 + o.0)
            }
        }

        impl Sub for $name {
            type Output = $name;
            fn sub(self, o: $name) -> $name {
                $name(self.0 - o.0)
            }
        }

        impl Neg for $name {
            type Output = $name;
            fn neg(self) -> $name {
                $name(-self.0)
            }
        }

        impl Mul<f64> for $name {
            type Output = $name;
            fn mul(self, k: f64) -> $name {
                $name(self.0 * k)
            }
        }

        impl Div<f64> for $name {
            type Output = $name;
            fn div(self, k: f64) -> $name {
                $name(self.0 / k)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{} {}", self.0, $unit)
            }
        }
    )*};
}

quantity_ops! {
    Mm: "mm";
    Radians: "rad";
}

impl Radians {
    #[must_use]
    pub fn from_degrees(deg: f64) -> Radians {
        Radians(deg.to_radians())
    }

    #[must_use]
    pub fn to_degrees(self) -> f64 {
        self.0.to_degrees()
    }
}

/// A position in space (mm).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Point(pub [f64; 3]);

impl Point {
    /// The distance to another point.
    #[must_use]
    pub fn distance(self, o: Point) -> Mm {
        let [a, b, c] = self.0;
        let [x, y, z] = o.0;
        Mm(((a - x).powi(2) + (b - y).powi(2) + (c - z).powi(2)).sqrt())
    }
}

/// A position in a sketch's plane (mm).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Point2(pub [f64; 2]);

/// A unit vector. Made from any finite vector with a length ([`Direction::new`]); read from JSON
/// the same way, so a zero or non-finite vector is refused.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 3]", into = "[f64; 3]")]
pub struct Direction([f64; 3]);

impl Direction {
    pub const X: Direction = Direction([1.0, 0.0, 0.0]);
    pub const Y: Direction = Direction([0.0, 1.0, 0.0]);
    pub const Z: Direction = Direction([0.0, 0.0, 1.0]);

    /// `v` scaled to unit length; `None` for a vector with no length or that is not finite.
    #[must_use]
    pub fn new(v: [f64; 3]) -> Option<Direction> {
        let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        (v.iter().all(|c| c.is_finite()) && n.is_finite() && n > 1e-300).then(|| Direction(v.map(|c| c / n + 0.0)))
    }

    #[must_use]
    pub fn get(self) -> [f64; 3] {
        self.0
    }
}

impl Neg for Direction {
    type Output = Direction;
    fn neg(self) -> Direction {
        Direction(self.0.map(|c| -c + 0.0))
    }
}

impl Default for Direction {
    fn default() -> Self {
        Direction::Z
    }
}

impl TryFrom<[f64; 3]> for Direction {
    type Error = String;

    fn try_from(v: [f64; 3]) -> Result<Self, String> {
        Direction::new(v).ok_or_else(|| format!("{v:?} is not a direction (a finite vector with a length)"))
    }
}

impl From<Direction> for [f64; 3] {
    fn from(d: Direction) -> Self {
        d.0
    }
}

/// A plane: the points `p` with `(p − point) · normal = 0`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Plane {
    pub point: Point,
    pub normal: Direction,
}

impl Plane {
    #[must_use]
    pub fn new(point: Point, normal: Direction) -> Plane {
        Plane { point, normal }
    }
}

/// A plane with axes: a sketch's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Frame {
    pub origin: Point,
    pub x: Direction,
    pub y: Direction,
    pub normal: Direction,
}

/// A rigid or mirroring transform as a row-major 4×4 matrix (the last column the translation, mm).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Transform(pub [[f64; 4]; 4]);

impl Default for Transform {
    fn default() -> Self {
        Transform([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]])
    }
}

/// A number with what it measures. JSON: `{"length": 3.0}`, `{"angle": 0.5}`, `{"number": 4}`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Quantity {
    Length(Mm),
    Angle(Radians),
    /// A count, a factor or another unit-less number.
    Number(f64),
}

impl From<Mm> for Quantity {
    fn from(l: Mm) -> Self {
        Quantity::Length(l)
    }
}

impl From<Radians> for Quantity {
    fn from(a: Radians) -> Self {
        Quantity::Angle(a)
    }
}

/// The length unit of a snapshot: always millimetres.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LengthUnit {
    #[default]
    Mm,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantities_add_scale_and_show_their_unit() {
        assert_eq!(Mm(2.0) + Mm(3.0) * 2.0 - Mm(1.0), Mm(7.0));
        assert_eq!(-Mm(2.0) / 4.0, Mm(-0.5));
        assert_eq!(Mm(3.5).to_string(), "3.5 mm");
        assert!((Radians::from_degrees(180.0).0 - std::f64::consts::PI).abs() < 1e-15);
        assert_eq!(Quantity::from(Mm(1.0)), Quantity::Length(Mm(1.0)));
        assert_eq!(Point([0.0, 3.0, 0.0]).distance(Point([4.0, 0.0, 0.0])), Mm(5.0));
    }

    #[test]
    fn a_direction_is_a_unit_vector_or_nothing() {
        assert_eq!(Direction::new([0.0, 0.0, 2.0]), Some(Direction::Z));
        assert_eq!((-Direction::X).get(), [-1.0, 0.0, 0.0]);
        for bad in [[0.0; 3], [f64::NAN, 0.0, 0.0], [f64::INFINITY, 0.0, 0.0]] {
            assert_eq!(Direction::new(bad), None, "{bad:?}");
        }
        assert!(serde_json::from_str::<Direction>("[0, 0, 0]").is_err());
        assert_eq!(serde_json::from_str::<Direction>("[0, 3, 4]").unwrap().get(), [0.0, 0.6, 0.8]);
    }
}
