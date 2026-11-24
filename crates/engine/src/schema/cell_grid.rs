//! Contract of `solid.cell_grid` (`cmd/cell_grid.rs`).

use schemars::JsonSchema;
use serde::Deserialize;

use super::types::{
    AngleExpr, DirectionParam, FeatureCommon, IdOrName, LengthExpr, NumberExpr, PointExpr2, PointExpr3, ProfilesParam, StringList, feature_common,
    present,
};

/// What the feature does with its cells.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CellGridModeParam {
    /// Cut the cells out of the bodies.
    Cut,
    /// Add the cells as material: they stand `depth` from the sketch plane in `direction`, joined
    /// to the bodies (a new body if there is none). Not with `through_all`.
    Raised,
}

/// A cell's shape. Every shape has its inscribed diameter `size`.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CellShapeParam {
    /// Regular hexagons, flat-top at angle 0.
    Hexagon,
    /// Axis-aligned squares.
    Square,
    /// Equilateral triangles pointing up and down alternately.
    Triangle,
    /// Circles, on a `dot_grid`.
    Circle,
    /// Regular polygons of `sides` sides, on a `dot_grid`.
    Polygon,
    /// Your own cell: the one closed profile of the sketch `cell_sketch`, repeated on a lattice
    /// of `pitch_x`, `pitch_y` and `stagger`. `size`, `sides` and `dot_grid` are not used, and
    /// `clip` defaults to `whole` (`snap` is refused).
    Sketch,
}

/// How circle and polygon cells' centres are laid out.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DotGridParam {
    /// Rows offset by half a pitch.
    Hexagonal,
    /// Rows and columns.
    Square,
}

/// What happens to a cell the field's edge crosses.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CellClipParam {
    /// Only cells entirely inside the field are cut.
    Whole,
    /// Cells next to an edge are cut along the grid's own lines, so the edge cells match the rest.
    Snap,
    /// Cells next to an edge are trimmed to the edge as drawn.
    Free,
}

/// Fill the closed profiles of a sketch with cells of one shape and cut them out of the bodies
/// (vents, speaker grilles, weight-saving pockets), or with `mode: raised` stand them on the
/// sketch plane as material (knurling, grip pads, standoffs). A profile's outer loop is the field to fill;
/// a loop inside it is a keep-out that no cell crosses. The cells are cut in one operation, up to
/// 1000 (the command help names the limit); a sheet metal body is refused (use
/// `sheet.perforate`). The faces cut have no stable names yet.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CellGridParams {
    /// The sketch holding the field, by id or name. Default: the active sketch, else the last one.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "IdOrName")]
    pub sketch: Option<IdOrName>,
    /// Which of the sketch's profiles hold the field and its keep-outs. Default: every drawn
    /// profile, a loop inside another being its keep-out.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "ProfilesParam")]
    pub profiles: Option<ProfilesParam>,
    /// What to do with the cells: cut them out, or raise them as material. Default: cut.
    #[serde(default)]
    pub mode: Option<CellGridModeParam>,
    /// The cells' shape. Default: hexagon. `sketch` repeats the profile of `cell_sketch`.
    #[serde(default)]
    pub shape: Option<CellShapeParam>,
    /// With `shape: sketch`, the sketch holding the cell, by id or name: exactly one closed
    /// profile without holes, of lines and arcs, drawn and constrained like any sketch.
    /// Required with `shape: sketch`, refused otherwise.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "IdOrName")]
    pub cell_sketch: Option<IdOrName>,
    /// With `shape: sketch`, the distance between cell origins along a row, at least 0.001 mm.
    /// Required with `shape: sketch`.
    #[serde(default)]
    pub pitch_x: Option<LengthExpr>,
    /// With `shape: sketch`, the distance between rows, at least 0.001 mm. Required with
    /// `shape: sketch`.
    #[serde(default)]
    pub pitch_y: Option<LengthExpr>,
    /// With `shape: sketch`, how far each row shifts along the row against the one before, as a
    /// fraction of `pitch_x`, from 0 to 1 (0.5: hexagonal and brick layouts). Default: 0.
    #[serde(default)]
    pub stagger: Option<NumberExpr>,
    /// With `shape: sketch`, the cell's turn about its own origin, apart from `angle`, which
    /// turns the whole grid. Default: 0.
    #[serde(default)]
    pub cell_angle: Option<AngleExpr>,
    /// With `shape: sketch`, mirror every other cell (those with row plus column odd) in the
    /// vertical axis through its origin, for cells that interlock. Default: false.
    #[serde(default)]
    pub flip: Option<bool>,
    /// With `shape: sketch`, the cell's origin in the cell sketch's own coordinates: the point
    /// that sits at each lattice point. Default: the profile's centroid.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "PointExpr2")]
    pub cell_origin: Option<PointExpr2>,
    /// Sides of a `polygon` cell: a whole number from 3 to 64. Default: 8.
    #[serde(default)]
    pub sides: Option<NumberExpr>,
    /// How the centres of `circle` and `polygon` cells are laid out. Default: hexagonal.
    #[serde(default)]
    pub dot_grid: Option<DotGridParam>,
    /// A cell's inscribed diameter, at least 0.001 mm. Required except with `shape: sketch`,
    /// where it is not used.
    #[serde(default)]
    pub size: Option<LengthExpr>,
    /// The wall between neighbouring cells, at least 0.001 mm.
    pub web: LengthExpr,
    /// The grid's rotation about the anchor. Default: 0.
    #[serde(default)]
    pub angle: Option<AngleExpr>,
    /// A cell's centre sits at this point, projected onto the sketch's plane: two grids with the
    /// same anchor, angle, size and web line up across parts. Default: the sketch's origin.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "PointExpr3")]
    pub anchor: Option<PointExpr3>,
    /// What happens to cells the field's edge crosses. Default: snap; `whole` with
    /// `shape: sketch`, which refuses `snap`.
    #[serde(default)]
    pub clip: Option<CellClipParam>,
    /// What happens to cells a keep-out's edge crosses (a loop inside the field). Default: the
    /// same as `clip`, so one mode serves every loop. Not `snap` with `shape: sketch`.
    #[serde(default)]
    pub keepout_clip: Option<CellClipParam>,
    /// With `clip: snap`, how many lattice lines further in each straight field edge moves after
    /// it snaps: a whole number, default 0. The edge cells become whole cells and the rim a
    /// whole number of grid rows thicker. Keep-outs are not moved.
    #[serde(default)]
    pub snap_inset: Option<NumberExpr>,
    /// The least clearance from a cell to every edge of the field and its keep-outs, with
    /// `clip: whole`. Default 0: a cell may touch an edge.
    #[serde(default)]
    pub margin: Option<LengthExpr>,
    /// How deep the cells are cut, or how high they stand when raised, along the sketch normal.
    /// Required unless `through_all`.
    #[serde(default)]
    pub depth: Option<LengthExpr>,
    /// Cut through every body in the way, instead of `depth`. Refused with `mode: raised`.
    #[serde(default)]
    pub through_all: Option<bool>,
    /// Which way to cut or raise. Default: positive (along the sketch normal).
    #[serde(default)]
    pub direction: Option<DirectionParam>,
    /// The bodies to cut, or to join the raised cells to. Default: the bodies of the active component (or `participants`).
    #[serde(default)]
    pub targets: Option<StringList>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

#[cfg(test)]
#[path = "cell_grid_tests.rs"]
mod tests;
