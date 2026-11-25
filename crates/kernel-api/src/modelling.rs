//! The body-making operations: primitives, extrude and revolve, booleans, blends and offsets
//! ([`Modelling`]), with their inputs.

use serde::{Deserialize, Serialize};
use solidcore_record::{Plane, Region2, Vec2, Vec3};

use crate::Backend;

/// Which boolean: the names are saved in designs, so they never change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoolOp {
    /// Material in either body.
    Union,
    /// The first body with the second taken away.
    Cut,
    /// Material in both bodies.
    Intersect,
}

/// How two constant rounds of one radius turn round a sharp, straight, convex side edge square to
/// both on a plane, such as the rounds along a boss's base at its upright edges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FilletTurn {
    /// The rounds run on to where they cross and meet in a mitre.
    #[default]
    Mitre,
    /// The rounds end at the side edge and the corner is their section revolved about it, a
    /// torus tangent to both. Refused where it is not built (unequal or variable rounds, a
    /// concave, curved or tangent side edge, a corner of four edges), with the reason.
    Swept,
}

/// The second side of an unequal chamfer: a distance along the other face, or the angle the
/// chamfer makes with the first face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChamferSide {
    /// The setback along the other face, in millimetres.
    Distance(f64),
    /// The angle the chamfer makes with the first face, in radians.
    Angle(f64),
}

/// A primitive solid, in millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Primitive {
    /// The axis-aligned box between two opposite corners.
    Box {
        /// One corner.
        a: Vec3,
        /// The opposite corner.
        b: Vec3,
    },
    /// A cylinder standing on `base`, along `axis`, `height` long.
    Cylinder {
        /// The centre of its base.
        base: Vec3,
        /// The direction it stands along.
        axis: Vec3,
        /// Its radius.
        radius: f64,
        /// Its length along `axis`.
        height: f64,
    },
    /// A sphere about `center`.
    Sphere {
        /// Its centre.
        center: Vec3,
        /// Its radius.
        radius: f64,
    },
    /// A ring torus about the z axis through `center`.
    Torus {
        /// Its centre.
        center: Vec3,
        /// The radius of the ring's centre line.
        major: f64,
        /// The radius of the ring's section.
        minor: f64,
    },
}

/// The axis a profile turns about, in the profile plane's coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis2 {
    /// A point on the axis.
    pub origin: Vec2,
    /// The axis's direction.
    pub dir: Vec2,
}

/// The body-making operations of one kernel ([`crate::OpKind`]'s primitives, `Extrude` to
/// `OffsetFaces`).
///
/// Edges and faces are picked by points, as the application picks them: the edge or face nearest
/// each point.
pub trait Modelling: Backend {
    /// A primitive solid.
    fn primitive(&self, p: &Primitive) -> Result<Self::Form, Self::Error>;
    /// `regions` on `plane`, extruded from `start` to `end` along its normal: one body per region
    /// that stays apart.
    fn extrude(&self, plane: &Plane, regions: &[Region2], start: f64, end: f64) -> Result<Vec<Self::Form>, Self::Error>;
    /// `regions` on `plane`, turned by `angle` (radians) about `axis`. `whole` asks for the whole
    /// revolution's form where the angle is a full turn.
    fn revolve(&self, plane: &Plane, regions: &[Region2], axis: Axis2, angle: f64, whole: bool) -> Result<Vec<Self::Form>, Self::Error>;
    /// `a` with `b` by `op`. `None` when nothing is left.
    fn boolean(&self, a: &Self::Form, b: &Self::Form, op: BoolOp) -> Result<Option<Self::Form>, Self::Error>;
    /// Constant-radius rounds of the edges nearest `edges`, turning at side edges as `turn` says.
    fn fillet(&self, body: &Self::Form, edges: &[Vec3], radius: f64, turn: FilletTurn) -> Result<Self::Form, Self::Error>;
    /// An equal chamfer of the edges nearest `edges`.
    fn chamfer(&self, body: &Self::Form, edges: &[Vec3], distance: f64) -> Result<Self::Form, Self::Error>;
    /// An unequal chamfer: `d1` along the first face (the one facing up most, the other with
    /// `flip`), the second side by distance or angle.
    fn chamfer_sides(&self, body: &Self::Form, edges: &[Vec3], d1: f64, side: ChamferSide, flip: bool) -> Result<Self::Form, Self::Error>;
    /// The body hollowed to `thickness`, open at the faces nearest `open`.
    fn shell(&self, body: &Self::Form, open: &[Vec3], thickness: f64) -> Result<Self::Form, Self::Error>;
    /// A sheet thickened to `thickness`, to both sides when `symmetric`.
    fn thicken(&self, sheet: &Self::Form, thickness: f64, symmetric: bool) -> Result<Self::Form, Self::Error>;
    /// The faces nearest `at` moved `distance` along their normals.
    fn offset_faces(&self, body: &Self::Form, at: &[Vec3], distance: f64) -> Result<Self::Form, Self::Error>;
}
