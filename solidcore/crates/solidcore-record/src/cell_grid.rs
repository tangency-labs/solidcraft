//! A cell grid as data: the parameters of a grid of cells over a sketch area, and the plan the
//! kernel returns (`solidcore::cell_grid`): the cells with their closed loops, the counts and the
//! open area. The field it is planned over is a list of [`Loop2`] (an outer loop is a field, a
//! loop inside it a keep-out, a loop inside that a field again). Plain data: the meaning of each
//! field, the lattice and the clipping are the kernel's.
use serde::{Deserialize, Serialize};

use crate::{Loop2, Vec2};

/// What a cell looks like. Every shape has its inscribed diameter `size`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CellShape {
    /// Regular hexagons, flat-top at angle 0.
    Hexagon,
    /// Axis-aligned squares.
    Square,
    /// Equilateral triangles pointing up and down alternately.
    Triangle,
    /// Circles on a [`DotGrid`].
    Circle,
    /// Regular polygons with `sides` sides on a [`DotGrid`].
    Polygon,
}

/// The grid of centres for circles and polygons.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DotGrid {
    /// Rows of centres offset by half a pitch.
    Hexagonal,
    /// Centres on a square grid.
    Square,
}

/// What happens to a cell the field's edge crosses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Clip {
    /// Only cells entirely inside the field.
    Whole,
    /// Edges move onto lattice lines; partial cells are cut along the cell's own chords.
    Snap,
    /// Cells are trimmed to the field as drawn.
    Free,
}

/// The grid's parameters, in the sketch's units (mm and radians).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CellGridSpec {
    /// The cell's shape.
    pub shape: CellShape,
    /// Sides of a polygon cell.
    pub sides: u32,
    /// The grid of centres for circles and polygons.
    pub dot_grid: DotGrid,
    /// The cell's inscribed diameter.
    pub size: f64,
    /// The gap between neighbouring cells.
    pub web: f64,
    /// The grid's rotation about the anchor.
    pub angle: f64,
    /// A cell centre sits here: this pins the lattice's phase.
    pub anchor: Vec2,
    /// What happens to a cell the field's edge crosses.
    pub clip: Clip,
    /// The least clearance from a cell's outline to every edge of the field (0: it may touch one),
    /// with `Clip::Whole`.
    #[serde(default)]
    pub margin: f64,
}

/// How a cell's opening came about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CellKind {
    /// The whole cell.
    Whole,
    /// A cell cut exactly along snapped edges.
    Exact,
    /// A cell next to an edge that could not snap, cut to the largest single-chord piece.
    Fallback,
    /// A cell trimmed to the field as drawn.
    Free,
}

/// One cell's opening.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GridCell {
    /// Lattice index along the first axis: with the parameters, `(i, j)` names the cell.
    pub i: i64,
    /// Lattice index along the second axis.
    pub j: i64,
    /// How the opening came about.
    pub kind: CellKind,
    /// The lattice centre of the cell, in the sketch plane.
    pub center: Vec2,
    /// The opening's closed loops: outer loops counter-clockwise, holes clockwise.
    pub loops: Vec<Loop2>,
    /// The opening's area (mm²).
    pub area: f64,
}

/// The cells to open, and what they add up to.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CellGridPlan {
    /// The cells.
    pub cells: Vec<GridCell>,
    /// Cells opened whole.
    pub whole: usize,
    /// Cells opened partially, fallback cells included.
    pub partial: usize,
    /// Partial cells next to an edge that could not snap.
    pub fallback: usize,
    /// The total area of the openings (mm²).
    pub open_area: f64,
    /// Things worth telling the user.
    pub warnings: Vec<String>,
}
