//! A sheet metal part's surfaces as data: its panels, each drawn in its own chart, the joints
//! where they meet and the reliefs cut where bends meet ([`Sheet`]), the rigid motions that place
//! them ([`Rigid2`], [`Rigid3`]), and what laying the sheet out flat gives ([`Development`]).
//!
//! Plain data with its serde form, as an application saves it: the kernel's `develop` module
//! lays a sheet out and folds it back (through `solidcore-ops`' `develop`), and
//! `solidcore-convert` copies these to the kernel's values and back, field by field. Nothing
//! here computes.

use serde::{Deserialize, Serialize};

use crate::{Region2, Seg2, Vec2, Vec3};

/// A sheet metal part's surfaces: the panels and the joints between them. The material lies on
/// the side of the sheet that a chart's `u × v` points to, `thickness` deep.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub thickness: f64,
    /// Where the neutral surface lies through the thickness, as a fraction of it from the
    /// inside of each bend.
    pub k_factor: f64,
    pub panels: Vec<Panel>,
    pub joints: Vec<Joint>,
    /// The reliefs cut where bends meet.
    pub corners: Vec<Corner>,
    /// The cuts that cross a joint (see [`Cut`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cuts: Vec<Cut>,
}

/// A cut through the sheet that crosses a joint, so that no one panel's region can hold it: a
/// hole across a bend's tangent line, or across a seam. Its region is drawn in the flat chart of
/// a panel it reaches (the chart the development places: a roll's neutral one), and it is cut
/// from every piece of the flat it reaches, bends' zones included. What it leaves of a zone bends
/// where it still joins both panels, and is a tab of the one panel it joins otherwise.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cut {
    pub panel: usize,
    pub region: Region2,
}

/// What is cut where two bends meet at a corner. A round or a square relief is cut from the flat
/// pattern wherever it reaches: both bends' zones and every panel round the corner alike, none of
/// them first. It is centered where the two zones' center lines cross, `size` across: a round,
/// or the four-sided relief whose sides run along the two bends, `size` apart. A notch or a tear
/// is the bends' own ends (see [`Relief`]), and cuts nothing more.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Corner {
    /// The two bend joints.
    pub joints: [usize; 2],
    pub relief: CornerRelief,
    pub size: f64,
}

/// The shape of a corner's relief (see [`Corner`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CornerRelief {
    Notch,
    Tear,
    Round,
    Square,
}

/// A face of the sheet. Its region is drawn in its surface's own parameters, so whatever is drawn
/// on the panel keeps its place on the surface, and the surface's flow (its parameter lines)
/// carries to the flat exactly. An empty region is a panel with no material of its own, which
/// still carries the joints on it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Panel {
    pub name: String,
    pub surface: Surface,
    pub region: Region2,
}

/// The surface a panel lies on, and so what its parameters are. A curved panel is a roll: its
/// radius is the panel's own geometry, where a bend's is a value of the joint. It flattens
/// through its neutral surface, as a bend does, so a roll and a bend of one radius give the same
/// flat. The parameters run so that the material lies on the side their `u x v` points to. Each
/// kind's flat has a closed form; a free-form surface's has none, and is no panel's.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Surface {
    /// A planar panel: its parameters are its plane's coordinates.
    #[default]
    Plane,
    /// A panel rolled on a cylinder of `radius` (the sheet's). Its parameters are length along
    /// the axis and angle round it (radians).
    Cylinder { radius: f64, material_inward: bool },
    /// A panel rolled on a cone whose rulings make `half_angle` (radians) with its axis. Its
    /// parameters are distance from the apex along a ruling and angle round the axis (radians).
    Cone {
        half_angle: f64,
        material_inward: bool,
    },
}

/// Where a joint lies on one of its panels: a straight line in the panel's parameters, through
/// `origin` along the unit `dir`. The two anchors of a joint are the same line on the sheet,
/// with the same origin and direction, and the `a` panel's material lies on the left of it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub panel: usize,
    pub origin: Vec2,
    pub dir: Vec2,
}

/// Two panels meeting along a line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Joint {
    pub a: Anchor,
    pub b: Anchor,
    pub kind: JointKind,
}

/// What a joint is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum JointKind {
    /// The sheet runs on from one panel to the other round a bend.
    Bend(Bend),
    /// The sheet runs on from one panel into the next with no bend between: where a roll
    /// meets a flat panel or another roll tangent to it.
    Seam,
    /// The panels are cut apart along their shared edge (see [`Rip`]), and the flat pattern lays
    /// them out apart unless other joints hold them together.
    Rip(Rip),
}

/// How a rip parts two panels along the edge they share, whatever the edge's course and the
/// panels' surfaces. At every point of the edge each panel is cut back from it, square to it on
/// its own surface, by as much as its lap asks there: so a rip between two planes, round a
/// roll's rim or along a curve across a cone is the one operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rip {
    /// The edge on each panel `(a, b)`, in its parameters: the same points of it, in order from
    /// the joint's start (its anchors' origin) to its end. Two make a straight edge.
    pub edges: (Vec<Vec2>, Vec<Vec2>),
    /// The edge on each panel `(a, b)` exactly, where its source knows it: lines, circular arcs
    /// and free-form spans in the panel's parameters, end to end from the joint's start to its
    /// end. Empty where only the points are known.
    pub curves: (Vec<Seg2>, Vec<Seg2>),
    /// How far the sheet turns across the edge at each of those points (radians, from the `a`
    /// panel to the `b` one, positive toward the material, as a bend turns): 0 where it would run
    /// straight on, so that a cut through a flat panel is a rip too.
    pub turns: Vec<f64>,
    pub lap: Lap,
}

/// How a rip parts its panels: what each is cut back from the edge, given how far the sheet
/// turns there and how thick it is.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Lap {
    /// Each panel's metal kept `gap / 2` off the surface halfway between the two, through its
    /// whole thickness: a mitered corner, or a slit `gap` wide through a flat panel.
    Miter { gap: f64 },
    /// Each panel cut back the given distance from the edge, `(a, b)`, whatever the turn.
    Cut(f64, f64),
}

/// A bend between two panels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bend {
    /// How far the sheet turns, in radians, more than 0 and at most a half turn.
    pub angle: f64,
    /// The inside radius.
    pub radius: f64,
    /// The bend turns toward the material side of the sheet, so the sheet runs round the
    /// outside of the bend; otherwise round the inside.
    pub toward_material: bool,
    /// What the anchors mark on the panels.
    pub datum: Datum,
    /// How long the edge is where the two panels meet, along the anchors from their origin: the
    /// zone's ends lie within it, where the corners and rips at its ends leave them.
    pub length: f64,
    /// The bend zone's ends along the joint.
    pub ends: ZoneEnds,
    /// What each panel keeps beyond the zone's ends, between its anchor and where the bend
    /// would start: `(a, b)`.
    pub tails: (Tail, Tail),
    /// What is cut at each of the zone's ends, `(start, end)` along the joint.
    pub relief: (Relief, Relief),
}

/// What is cut at one end of a bend's zone, from both its panels alike. Reliefs are the bend's,
/// never drawn on its panels: they move with its radius, its thickness and its ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Relief {
    /// Nothing: past the end each panel keeps or trims its material as its tail says, so a kept
    /// tail is torn from the bend along the end.
    #[default]
    None,
    /// Past the end both panels stop at their tangent lines: where the bend meets another at a
    /// corner, where a rip cuts the panels beside it, or at a free end whose zone carries on both
    /// panels' outlines, neither panel running on past it.
    Notch,
    /// A bend relief in each panel that runs on past the end: a slot `width` along past the
    /// end, cut from `depth` inside the panel's tangent line out through its edge. None is cut
    /// in a panel that runs on less far than that.
    Slot { width: f64, depth: f64 },
    /// Torn tight against a surface across the bend, as where a rip parts the panels beyond a
    /// corner: past the end both panels stop at their tangent lines, as at a notch, and the
    /// zone's end runs from its given end on the `a` panel's tangent line to its given end on
    /// the `b` panel's, its metal kept off the surface by as much as at those two ends and, in
    /// between, in proportion to how far the bend has turned, so that it follows the surface
    /// all the way round. The surface is the plane of the points `p` with `p · normal = offset`
    /// in the bend's frame at its `a` anchor (`x` along the joint, `y` toward the material, `z`
    /// toward the `b` panel), `normal` toward the bend's side; a lap or a free-form rip will
    /// part the panels along other surfaces.
    Tear { normal: Vec3, offset: f64 },
}

/// The line the anchors mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Datum {
    /// The virtual sharp, where the two panels' surfaces would meet if the bend had no radius.
    /// Changing the radius moves neither panel. A half turn has no sharp.
    Sharp,
    /// The tangent line, where each panel's flat material ends and the bend starts. A hem,
    /// whose panels never meet, is anchored here.
    Tangent,
}

/// The bend zone along its joint, in lengths along the anchors from their origin: where its
/// edge on the `a` side starts and ends (`near`), and where its edge on the `b` side does
/// (`far`). A square end has the same length in both; a torn end does not.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ZoneEnds {
    pub near: (f64, f64),
    pub far: (f64, f64),
}

/// What a panel keeps beyond the bend zone's ends, between its anchor and its tangent line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tail {
    /// The panel's own outline: a base that runs on beside a shorter flange keeps its edge.
    Keep,
    /// Nothing: past the zone's ends the panel starts where the bend would.
    Trim,
}

/// A rigid motion of the plane: a turn about the origin (its cosine and sine), then a shift. It
/// never reflects, so a panel keeps its handedness and an arc its sense of turning.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rigid2 {
    pub cos: f64,
    pub sin: f64,
    pub shift: Vec2,
}

impl Rigid2 {
    /// The motion that moves nothing.
    pub const IDENTITY: Rigid2 = Rigid2 {
        cos: 1.0,
        sin: 0.0,
        shift: Vec2::ZERO,
    };
}

/// A rigid motion of space: a turn that puts the axes on `x`, `y` and `z` (unit vectors, square
/// to each other and right-handed), then a shift. It never reflects.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rigid3 {
    pub x: Vec3,
    pub y: Vec3,
    pub z: Vec3,
    pub shift: Vec3,
}

impl Rigid3 {
    /// The motion that moves nothing.
    pub const IDENTITY: Rigid3 = Rigid3 {
        x: Vec3::X,
        y: Vec3::Y,
        z: Vec3::Z,
        shift: Vec3::ZERO,
    };
}

/// A sheet laid out flat.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Development {
    /// Where each panel's neutral chart lies in the flat frame (a planar panel's neutral chart
    /// is its chart).
    pub placements: Vec<Option<Rigid2>>,
    /// Each panel with material, in the flat frame.
    pub panels: Vec<Option<FlatPanel>>,
    /// The pieces of the flat pattern: the panels that bends and seams hold together. The pieces
    /// with an anchored panel come first and lie where their anchors put them; the others are
    /// laid beside them, left to right.
    pub pieces: Vec<Vec<usize>>,
    /// Each bend's zone in the flat frame.
    pub zones: Vec<Zone>,
    /// What to take out of each panel before the pieces are joined: its material between its
    /// tangent line and a bend's anchor, along the zone and, where its tail is trimmed, beyond
    /// it. A rip's cut is no trim: it is in its panels' outlines (see [`FlatPanel`]).
    pub trims: Vec<Trim>,
    /// Bends and seams that close a loop, where the layout reached both panels another way.
    pub loops: Vec<Mismatch>,
    /// What is cut from the flat, from every piece it reaches: the corners' reliefs, then the
    /// sheet's cuts.
    pub cuts: Vec<Region2>,
    /// The bend ends a panel runs on past where the bend's relief has no width to cut: the
    /// bend's joint, the panel, and where along the joint the end lies.
    pub unrelieved: Vec<(usize, usize, f64)>,
}

/// A panel laid flat, its rips' cuts taken off. For each loop of its region, `origins` gives what
/// each segment is in the sheet (see [`EdgeFrom`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlatPanel {
    pub region: Region2,
    pub origins: Vec<Vec<EdgeFrom>>,
}

/// What an edge of a flat panel is in its sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeFrom {
    /// Segment `k` of the same loop of the panel's own region, or what a rip left of it.
    Own(usize),
    /// The panel's new edge along joint `j`'s rip, with what joins it to the edges beside it.
    Rip(usize),
}

/// A bend's zone in the flat frame: its region, its edge on the `a` panel's side (`near`) and on
/// the `b` panel's side (`far`), each from the end at the anchors' origin to the other, and the
/// panels on those sides. Its ends join `near[k]` to `far[k]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    pub joint: usize,
    pub region: Region2,
    pub near: [Vec2; 2],
    pub far: [Vec2; 2],
    pub panels: [usize; 2],
}

/// What a joint takes from a panel: the region to cut, and the line along which the panel's
/// material ends once it is cut (its tangent line at a bend, its cut edge at a rip).
#[derive(Clone, Debug, PartialEq)]
pub struct Trim {
    pub panel: usize,
    pub joint: usize,
    pub region: Region2,
    pub line: [Vec2; 2],
}

/// How far a joint that closes a loop is from fitting: laid out through the rest of the loop,
/// its second panel lands `distance` (mm) and `angle` (radians) from where the joint itself
/// would put it. A ring of bends and seams never fits; it needs a rip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mismatch {
    pub joint: usize,
    pub distance: f64,
    pub angle: f64,
}
