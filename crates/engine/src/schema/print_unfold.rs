//! Contracts and result types for the commands in `cmd/print_unfold.rs`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::types::Point3;

/// Copy a body into a new design, lying on one of its flat faces, to unfold it there for
/// printing. The new design replaces the session's; the app opens it in a tab of its own.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct StartParams {
    /// The body, by name. Every face of it must be flat.
    pub body: String,
    /// A point on the face it will lie on (the base face), in mm, in the design's coordinates.
    pub face: Point3,
}

/// Set a bend at an edge of the body being unfolded, or clear the bend that is there.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct BendParams {
    /// A point on the edge, in mm.
    pub edge: Point3,
}

/// Carve faces out of the pattern into a group: a part of its own.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct GroupParams {
    /// A point on each face, in mm (1 to 200 faces). The part starts at the first.
    pub faces: Vec<Point3>,
}

/// Put the faces of a carved-out group back.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct UngroupParams {
    /// A point on a face of the group, in mm.
    pub face: Point3,
}

/// Choose the print bed the flat pattern is checked against.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct BedParams {
    /// The printer's name. Default: none, a bed of the size given.
    #[serde(default)]
    pub name: Option<String>,
    /// The bed's width and depth, in mm (10 to 5000).
    pub width: f64,
    pub depth: f64,
}

/// Split the flat pattern at a bend, or join it there again.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SplitParams {
    /// A point on the bend in the flat pattern, in mm.
    pub edge: Point3,
}

/// Choose the wall thickness and what the bends and cuts of the print get.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct JointsParams {
    /// The wall thickness, a length (default: as it is, 2 mm at first).
    #[serde(default)]
    pub wall: Option<String>,
    /// What every bend is: `film`, `filament` or `press` (default: as it is, `film` at first).
    #[serde(default)]
    pub bends: Option<String>,
    /// How walls meet at every cut: `butt`, `mitre`, `filament` or `press` (default: as it is,
    /// `butt` at first).
    #[serde(default)]
    pub cuts: Option<String>,
}

/// Give one edge of the flat pattern a joint of its own.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct JointParams {
    /// A point on the edge in the flat pattern, in mm.
    pub edge: Point3,
    /// `plain` (a bend's film, a cut's butt or mitre), `filament`, `press`, or `default` to
    /// follow what all bends or cuts get again.
    pub kind: String,
}

/// A command of the unfolded print that takes no parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct NoParams {}

/// An edge of the body being unfolded.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct PrintUnfoldEdge {
    /// Its ends, in mm.
    pub start: [f64; 3],
    pub end: [f64; 3],
    /// The two faces it lies between (indices of the body's faces). A cut is an edge of each of
    /// them in the pattern: the two edges are a pair.
    pub faces: [usize; 2],
    /// The angle a bend here folds by, in degrees. Absent where the edge can't fold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<f64>,
    /// Whether the edge can be a bend: its faces meet round the outside of the body, or in an
    /// inside corner between two faces of one carved-out group.
    pub foldable: bool,
}

/// The print bed the flat pattern is checked against.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct PrintUnfoldBed {
    /// The printer's name (empty for a size typed in).
    pub name: String,
    pub width: f64,
    pub depth: f64,
}

/// A part of the flat pattern.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct PrintUnfoldPart {
    /// How many faces it has.
    pub faces: usize,
    /// The longer and the shorter side of the smallest rectangle round it, in mm.
    pub long: f64,
    pub short: f64,
    /// That rectangle's corners (the one that fits the bed, when one does), in mm.
    pub corners: [[f64; 2]; 4],
    /// Whether a rectangle round it fits the bed. Absent while no bed is chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fits: Option<bool>,
    /// How many pairs of its faces lie on each other: split it so they print apart.
    pub overlaps: usize,
}

/// An edge of the flat pattern.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct PrintUnfoldFlatEdge {
    /// Its ends in the flat pattern, in mm.
    pub start: [f64; 3],
    pub end: [f64; 3],
    /// The body's edge it is: the two flat edges of a cut or a split bend share it (a pair).
    pub edge: usize,
    /// A bend the pattern is split at.
    pub split: bool,
    /// The joint it gets: `film`, `butt`, `mitre`, `filament` or `press`.
    pub joint: String,
    /// Whether that joint is the edge's own, not what all bends or cuts get.
    pub own: bool,
}

/// What the commands of the unfolded print return: how far it has come and what every edge is.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PrintUnfoldState {
    /// The body being unfolded.
    pub body: String,
    /// `edges`: the bends and cuts are being set. `surface`: the surface model is made. `flat`: it
    /// is laid flat. `solid`: the printable body is made.
    pub step: String,
    /// The base face (an index of the body's faces).
    pub base: usize,
    /// How many faces the body has, and how many of them hang on the pattern.
    pub faces: usize,
    pub attached: usize,
    /// Whether every face hangs on the pattern, so every edge is a bend or a cut.
    pub complete: bool,
    /// The edges the pattern folds at.
    pub bends: Vec<PrintUnfoldEdge>,
    /// The edges the pattern is cut open at.
    pub cuts: Vec<PrintUnfoldEdge>,
    /// The edges neither face of which hangs on the pattern yet.
    pub open: Vec<PrintUnfoldEdge>,
    /// The outlines of the faces that do not hang on the pattern yet: every loop of each (its
    /// outer one and its holes) as its corners, in mm.
    pub missing: Vec<Vec<[f64; 3]>>,
    /// How many groups of faces are carved out of the pattern, and the outlines of their faces
    /// (every loop of each as its corners, in mm, where the faces lie on the body).
    pub groups: usize,
    pub carved: Vec<Vec<[f64; 3]>>,
    /// A point inside the first face of each carved-out group, in mm.
    pub group_points: Vec<[f64; 3]>,
    /// The wall thickness and what all bends and all cuts get.
    pub wall: String,
    /// The least wall thickness the joints chosen need, in mm: a filament channel and the film
    /// either side, a press-fit nub and the film either side. 0 while the joints are the film
    /// and butt or mitre alone.
    pub min_wall: f64,
    pub bend_joint: String,
    pub cut_joint: String,
    /// The print bed chosen. Absent while none is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bed: Option<PrintUnfoldBed>,
    /// Laid flat: the parts of the pattern, the bends where it still folds, and the edges it is
    /// open at (cuts and split bends, each as the edge of either face).
    pub parts: Vec<PrintUnfoldPart>,
    pub flat_bends: Vec<PrintUnfoldFlatEdge>,
    pub flat_cuts: Vec<PrintUnfoldFlatEdge>,
    /// Laid flat: the outlines of the faces that lie on another face of their part (every loop
    /// of each as its corners, in mm). While there are any, the pattern can't be printed.
    pub overlapping: Vec<Vec<[f64; 3]>>,
    /// What `print.unfold.bend` did to its edge: `added`, `moved` (the face hung by another
    /// bend before) or `removed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed: Option<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
