//! The body-making operations: primitives, extrude and revolve, booleans, blends and offsets
//! ([`Modelling`]), with their inputs.

use serde::{Deserialize, Serialize};
use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};

use crate::Backend;
use crate::sweep::{Helix, PathSpan, PointPath, RailFit, SweepOptions};

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

/// What splits a body's faces, with `F` the kernel's body.
pub enum SplitTool<'a, F> {
    /// A plane (unbounded).
    Plane(Plane),
    /// The surface of another body's face (`face` in that body's face order), extended. Split
    /// Body by a face of a surface body joined tangentially to others cuts with their smooth
    /// sheet as it is, or with the whole sheet where that does not cut through.
    Face {
        /// The body the face is on.
        body: &'a F,
        /// The face, in that body's face order.
        face: usize,
    },
    /// Sketch curves (chains of segments in `plane`) swept along `along` (none: the plane's
    /// normal); each curve splits on its own, an open one extended past its ends.
    Curves {
        /// The sketch plane.
        plane: Plane,
        /// The chains of segments.
        curves: Vec<Vec<Seg2>>,
        /// The direction they are swept along.
        along: Option<Vec3>,
    },
}

impl<F> Clone for SplitTool<'_, F> {
    fn clone(&self) -> Self {
        match self {
            SplitTool::Plane(p) => SplitTool::Plane(*p),
            SplitTool::Face { body, face } => SplitTool::Face { body, face: *face },
            SplitTool::Curves { plane, curves, along } => SplitTool::Curves { plane: *plane, curves: curves.clone(), along: *along },
        }
    }
}

impl<F: std::fmt::Debug> std::fmt::Debug for SplitTool<'_, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SplitTool::Plane(p) => f.debug_tuple("Plane").field(p).finish(),
            SplitTool::Face { body, face } => f.debug_struct("Face").field("body", body).field("face", face).finish(),
            SplitTool::Curves { plane, curves, along } => {
                f.debug_struct("Curves").field("plane", plane).field("curves", curves).field("along", along).finish()
            }
        }
    }
}

/// What a trim keeps.
#[derive(Clone, Copy, Debug)]
pub enum TrimKeep<'a> {
    /// Every piece on the side of the tool this point is on. A tool of several sketch curves
    /// has no one side: the region the point picks is kept.
    Side(Vec3),
    /// The regions the points pick (a region: the pieces that hang together once the tool has
    /// cut the body, each picked by the piece nearest a point).
    Regions(&'a [Vec3]),
}

impl TrimKeep<'_> {
    /// The points that pick what is kept.
    pub fn points(&self) -> &[Vec3] {
        match self {
            TrimKeep::Side(p) => std::slice::from_ref(p),
            TrimKeep::Regions(at) => at,
        }
    }
}

/// The body-making operations of one kernel ([`crate::OpKind`]'s primitives, `Extrude` to
/// `Split`).
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
    /// [`Modelling::fillet`], with the picks whose edges are gone: where some rounds overflow
    /// their faces those are built first, and a picked edge one of them removes whole is not
    /// rounded; a single concave round running off the flat face it rolls on carries that face
    /// on. The picks left out come back with the body.
    fn fillet_removing(&self, body: &Self::Form, edges: &[Vec3], radius: f64, turn: FilletTurn) -> Result<(Self::Form, Vec<Vec3>), Self::Error>;
    /// Each edge nearest `edges` rounded at its own radius (`radii` in `edges`' order), in one
    /// blend.
    fn fillet_each(&self, body: &Self::Form, edges: &[Vec3], radii: &[f64], turn: FilletTurn) -> Result<Self::Form, Self::Error>;
    /// Constant-radius rounds set back from the corners at the vertices nearest each point by
    /// its distance, with a patch between the rounds there.
    fn fillet_setback(&self, body: &Self::Form, edges: &[Vec3], radius: f64, setbacks: &[(Vec3, f64)]) -> Result<Self::Form, Self::Error>;
    /// An equal chamfer of the edges nearest `edges`.
    fn chamfer(&self, body: &Self::Form, edges: &[Vec3], distance: f64) -> Result<Self::Form, Self::Error>;
    /// [`Modelling::chamfer`], and where a single concave chamfer runs off the flat face it
    /// rolls on, that face carried on.
    fn chamfer_carrying_face(&self, body: &Self::Form, edges: &[Vec3], distance: f64) -> Result<Self::Form, Self::Error>;
    /// An unequal chamfer: `d1` along the first face (the one facing up most, the other with
    /// `flip`), the second side by distance or angle.
    fn chamfer_sides(&self, body: &Self::Form, edges: &[Vec3], d1: f64, side: ChamferSide, flip: bool) -> Result<Self::Form, Self::Error>;
    /// [`Modelling::chamfer_sides`], carrying a flat face on as [`Modelling::chamfer_carrying_face`].
    fn chamfer_sides_carrying_face(
        &self,
        body: &Self::Form,
        edges: &[Vec3],
        d1: f64,
        side: ChamferSide,
        flip: bool,
    ) -> Result<Self::Form, Self::Error>;
    /// The body hollowed to `thickness`, open at the faces nearest `open`.
    fn shell(&self, body: &Self::Form, open: &[Vec3], thickness: f64) -> Result<Self::Form, Self::Error>;
    /// A sheet thickened to `thickness`, to both sides when `symmetric`.
    fn thicken(&self, sheet: &Self::Form, thickness: f64, symmetric: bool) -> Result<Self::Form, Self::Error>;
    /// The faces nearest `at` moved `distance` along their normals.
    fn offset_faces(&self, body: &Self::Form, at: &[Vec3], distance: f64) -> Result<Self::Form, Self::Error>;
    /// Rotate one face's support by `angle` radians about the axis through `origin`, then
    /// translate it (mm). Adjacent faces stay on their own supports.
    fn transform_face(&self, body: &Self::Form, at: Vec3, translate: Vec3, origin: Vec3, axis: Vec3, angle: f64) -> Result<Self::Form, Self::Error>;
    /// The faces nearest `faces` tilted by `angle` about their line on the `neutral` plane, so
    /// that they lean in (positive angle) going along `pull`.
    fn draft(&self, body: &Self::Form, faces: &[Vec3], neutral: &Plane, pull: Vec3, angle: f64) -> Result<Self::Form, Self::Error>;
    /// The faces `faces` (in the body's face order) split where `tool` meets them, the solid
    /// unchanged, with the body face each face of the result came from.
    fn split_faces(&self, body: &Self::Form, faces: &[usize], tool: &SplitTool<'_, Self::Form>) -> Result<(Self::Form, Vec<usize>), Self::Error>;
    /// The body split into the closed parts on either side of `tool`, those on its back first.
    fn split_body(&self, body: &Self::Form, tool: &SplitTool<'_, Self::Form>) -> Result<Vec<Self::Form>, Self::Error>;
    /// The body split by `sheet`'s faces `faces` exactly as they are, never extended, `face`
    /// giving the sides; refused unless they cut it into closed solids.
    fn split_body_by_faces(&self, body: &Self::Form, sheet: &Self::Form, face: usize, faces: &[usize]) -> Result<Vec<Self::Form>, Self::Error>;
    /// The body split by every face of the surface body `sheet` exactly as it is, where that
    /// cuts it into closed solids (`None` where it doesn't), `face` giving the sides.
    fn split_body_by_sheet_where_it_cuts(&self, body: &Self::Form, sheet: &Self::Form, face: usize) -> Result<Option<Vec<Self::Form>>, Self::Error>;
    /// A surface body cut where `tool` meets it, keeping what `keep` says.
    fn trim_keep(&self, body: &Self::Form, tool: &SplitTool<'_, Self::Form>, keep: TrimKeep<'_>) -> Result<Self::Form, Self::Error>;
    /// A planar region on `plane` swept along `path` (segments end to end), as `options` say.
    fn sweep(&self, plane: &Plane, region: &Region2, path: &[PathSpan], options: &SweepOptions) -> Result<Self::Form, Self::Error>;
    /// A planar region swept along a path through 3D sketch curves, as `options` say.
    fn sweep_points(&self, plane: &Plane, region: &Region2, path: &PointPath, options: &SweepOptions) -> Result<Self::Form, Self::Error>;
    /// A planar region's outer loop swept along a helix.
    fn sweep_helix(&self, plane: &Plane, region: &Region2, helix: &Helix<'_>) -> Result<Self::Form, Self::Error>;
    /// A solid through closed planar sections in order, following 0 to 16 guide `rails`
    /// (polylines from the first section to the last), `fit` saying how they reshape the
    /// sections between.
    fn loft(&self, sections: &[(Plane, Loop2)], rails: &[Vec<Vec3>], fit: RailFit) -> Result<Self::Form, Self::Error>;
    /// A loft through regions with matching hole counts: the hole walls share the end caps.
    fn loft_regions(&self, sections: &[(Plane, Region2)]) -> Result<Self::Form, Self::Error>;
    /// A solid from a planar loop to an apex off its plane, following 0 to 2 guide `rails`.
    fn loft_to_point(&self, plane: &Plane, section: &Loop2, apex: Vec3, rails: &[Vec<Vec3>], fit: RailFit) -> Result<Self::Form, Self::Error>;
}
