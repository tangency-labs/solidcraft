//! Contract of `solid.cell_grid` (`cmd/cell_grid.rs`).

use schemars::JsonSchema;
use serde::Deserialize;

use super::types::{
    AngleExpr, DirectionParam, FeatureCommon, IdOrName, LengthExpr, NumberExpr, PointExpr3, ProfilesParam, StringList, feature_common, present,
};

/// What the feature does with its cells. Only `cut` is built so far.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CellGridModeParam {
    /// Cut the cells out of the bodies.
    Cut,
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

/// Fill the closed profiles of a sketch with cells of one shape and cut them out of the bodies:
/// vents, speaker grilles, weight-saving pockets. A profile's outer loop is the field to fill;
/// a loop inside it is a keep-out that no cell crosses. The cells are cut in one operation, a few
/// hundred at most (the command help names the limit); a sheet metal body is refused (use
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
    /// What to do with the cells. Default: cut.
    #[serde(default)]
    pub mode: Option<CellGridModeParam>,
    /// The cells' shape. Default: hexagon.
    #[serde(default)]
    pub shape: Option<CellShapeParam>,
    /// Sides of a `polygon` cell: a whole number from 3 to 64. Default: 8.
    #[serde(default)]
    pub sides: Option<NumberExpr>,
    /// How the centres of `circle` and `polygon` cells are laid out. Default: hexagonal.
    #[serde(default)]
    pub dot_grid: Option<DotGridParam>,
    /// A cell's inscribed diameter, at least 0.001 mm.
    pub size: LengthExpr,
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
    /// What happens to cells the field's edge crosses. Default: snap.
    #[serde(default)]
    pub clip: Option<CellClipParam>,
    /// How deep the cells are cut along the sketch normal. Required unless `through_all`.
    #[serde(default)]
    pub depth: Option<LengthExpr>,
    /// Cut through every body in the way, instead of `depth`.
    #[serde(default)]
    pub through_all: Option<bool>,
    /// Which way to cut. Default: positive (along the sketch normal).
    #[serde(default)]
    pub direction: Option<DirectionParam>,
    /// The bodies to cut. Default: the bodies of the active component (or `participants`).
    #[serde(default)]
    pub targets: Option<StringList>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

#[cfg(test)]
#[path = "cell_grid_tests.rs"]
mod tests;
