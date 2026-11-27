//! The body-making operations: primitives, extrude and revolve, booleans, blends and offsets
//! ([`Modelling`]), with their inputs.

use serde::{Deserialize, Serialize};
use solidcore_record::{Plane, Region2, Seg2, Vec2, Vec3};

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

/// How two rounds turn where they meet at a sharp side edge on a face they share, such as the
/// rounds along a boss's base at its upright edges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FilletTurn {
    /// The rounds run on to where they cross and meet in a mitre.
    #[default]
    Mitre,
    /// The rounds end at the side edge and the corner is their section revolved about it, a
    /// torus tangent to both: the rolling ball kept on the side edge. Refused where it is not
    /// built, with the reason: a side edge of the rounds' own convexity (the ball cannot pass
    /// it), unequal or variable rounds, a curved or tangent side edge, a corner of four edges.
    Swept,
    /// Each turn swept where it is built, as [`FilletTurn::Swept`], and mitred where it is not:
    /// decided per corner before anything is built, so never refused where a mitre is made.
    Auto,
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

/// Whether two neighbouring free-form segments of a profile (conics, Béziers) are pieces of one
/// curve, such as an ellipse's quarters or a spline's spans.
pub type OneCurve<'a> = &'a dyn Fn(&Seg2, &Seg2) -> bool;

/// How an extrusion's side faces follow its profile's segments.
#[derive(Clone, Copy)]
pub enum ExtrudeSides<'a> {
    /// A side face per segment.
    AsDrawn,
    /// Each run of lines that continue one another in a straight line swept into one side face,
    /// and each run of free-form segments the predicate calls one curve swept as that curve into
    /// one wall; the side's history lists every piece under it.
    Whole(OneCurve<'a>),
}

impl std::fmt::Debug for ExtrudeSides<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ExtrudeSides::AsDrawn => "AsDrawn",
            ExtrudeSides::Whole(_) => "Whole",
        })
    }
}

/// How a revolution's faces follow its profile's segments.
#[derive(Clone, Copy)]
pub enum RevolveSides<'a> {
    /// A face per segment.
    AsDrawn,
    /// Contiguous pieces of one circle joined before the profile turns.
    JoinArcs,
    /// Each run of free-form segments the predicate calls one curve turned as that curve into
    /// one face.
    JoinFreeForm(OneCurve<'a>),
}

impl std::fmt::Debug for RevolveSides<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            RevolveSides::AsDrawn => "AsDrawn",
            RevolveSides::JoinArcs => "JoinArcs",
            RevolveSides::JoinFreeForm(_) => "JoinFreeForm",
        })
    }
}

/// How a tapered extrusion closes the gap its offset opens at a sharp corner of the profile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftCorner {
    /// Each wall continues as itself (a line's plane, an arc's cone) until it meets its
    /// neighbour: every section is the profile's curves offset and continued as themselves.
    #[default]
    PreserveCurvature,
    /// Each offset curve continues straight along its end tangent (a flat face each side).
    Tangent,
    /// An arc about the corner (a cone about it).
    Round,
}

impl DraftCorner {
    /// The three rules, in the order a dialog offers them.
    pub const ALL: [DraftCorner; 3] = [DraftCorner::PreserveCurvature, DraftCorner::Tangent, DraftCorner::Round];
}

/// A tapered extrusion of one region: from `from` to `to` along `dir_sign` × the plane's normal,
/// the region being the section at 0, its sides sloping by `angle` (radians, positive grows the
/// section). With a `foot` (a height, less than `to - from`) the sides stand straight for that
/// height above `from` and only then slope.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Taper {
    /// Where the solid starts, along the direction.
    pub from: f64,
    /// Where it ends.
    pub to: f64,
    /// The straight height above `from` before the sides slope.
    pub foot: f64,
    /// `1` along the plane's normal, `-1` against it.
    pub dir_sign: f64,
    /// The sides' slope, in radians.
    pub angle: f64,
    /// How sharp corners close.
    pub corner: DraftCorner,
}

/// The body-making operations of one kernel ([`crate::OpKind`]'s primitives, `Extrude` to
/// `OffsetFaces`).
///
/// Edges and faces are picked by points, as the application picks them: the edge or face nearest
/// each point.
pub trait Modelling: Backend {
    /// A primitive solid.
    fn primitive(&self, p: &Primitive) -> Result<Self::Form, Self::Error>;
    /// `regions` on `plane`, extruded from `start` to `end` along its normal: one body per
    /// region that stays apart, with side faces as `sides` says.
    fn extrude(&self, plane: &Plane, regions: &[Region2], start: f64, end: f64, sides: ExtrudeSides<'_>) -> Result<Vec<Self::Form>, Self::Error>;
    /// The material of `regions` on `plane` (overlapping ones merged) extruded from `start` to
    /// `end` along its normal, as one body.
    fn extrude_material(&self, plane: &Plane, regions: &[Region2], start: f64, end: f64) -> Result<Self::Form, Self::Error>;
    /// One region on `plane` extruded with sloping sides ([`Taper`]).
    fn extrude_tapered(&self, plane: &Plane, region: &Region2, taper: &Taper) -> Result<Self::Form, Self::Error>;
    /// `regions` on `plane`, turned by `angle` (radians) about `axis`. `whole` asks for the whole
    /// revolution's form where the angle is a full turn; `sides` how the faces follow the profile.
    fn revolve(
        &self,
        plane: &Plane,
        regions: &[Region2],
        axis: Axis2,
        angle: f64,
        whole: bool,
        sides: RevolveSides<'_>,
    ) -> Result<Vec<Self::Form>, Self::Error>;
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
