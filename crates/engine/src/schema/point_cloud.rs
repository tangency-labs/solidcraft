//! Contracts and result types for the commands in `cmd/point_cloud.rs` that have them.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use solidcraft_doc::point_cloud::{Filter, MAX_COORD, MAX_FILTERS, MAX_NAMES, POINT_SIZE};

use super::check::whole;
use super::component::{MatrixParam, MatrixTolerance, refusal};
use super::file::{CloudUnits, MaxPoints, OpenPath};
use super::types::IdOrName;

/// Insert a point cloud: the points of a `.ply`, `.xyz` or `.pts` file join the design as a
/// reference to model from. They are drawn as points and never become a body; a PLY's red, green
/// and blue become the cloud's colours, its other properties can colour and filter the points, and
/// its edge and face elements become segments and triangles.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PointCloudInsertParams {
    /// The file to read: a `.ply`, `.xyz` or `.pts` file. Any other file is refused.
    pub path: OpenPath,
    /// The cloud's name in the design. Default: the file's name without its extension, with
    /// ` (2)` and so on added when another cloud already has that name.
    #[serde(default)]
    pub name: Option<String>,
    /// The file's length unit: the points are scaled from the unit they came in as to mm.
    /// Default: mm.
    #[serde(default)]
    pub units: Option<CloudUnits>,
    /// The most points kept; a file with more is thinned evenly. Default: 5000000.
    #[serde(default)]
    pub max_points: Option<MaxPoints>,
    /// `true` moves the cloud so the middle of its base sits at the origin. Default: false.
    #[serde(default)]
    pub center: Option<bool>,
    /// The size of a point on screen, in pixels. Default: 2.
    #[serde(default)]
    pub point_size: Option<CloudPointSize>,
    /// How the points are coloured. Default: the file's colours.
    #[serde(default)]
    pub color_by: Option<CloudColorBy>,
    /// The one colour of every point. Given on its own it colours the points with it; `null`
    /// means the file's colours.
    #[serde(default, deserialize_with = "color_or_file")]
    #[schemars(schema_with = "color_schema")]
    pub color: Option<Option<CloudColor>>,
    /// The range the colour ramp runs between. `null` works the range out.
    #[serde(default, deserialize_with = "range_or_auto")]
    #[schemars(schema_with = "range_schema")]
    pub color_range: Option<Option<CloudRange>>,
    /// The same as `color_range`, which is read first when both are given.
    #[serde(default, deserialize_with = "range_or_auto")]
    #[schemars(schema_with = "range_schema")]
    pub height_range: Option<Option<CloudRange>>,
    /// Which elements are shown: only what every filter keeps. `null` means no filters.
    #[serde(default, deserialize_with = "filters_or_default")]
    #[schemars(with = "CloudFilters")]
    pub filters: Option<Option<CloudFilters>>,
}

/// What `pointcloud.insert` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PointCloudInsertResult {
    /// The cloud's id.
    pub cloud: u64,
    /// Its name in the design.
    pub name: String,
    /// How many points it has.
    pub points: usize,
    /// How many segments it has.
    pub segments: usize,
    /// How many triangles it has.
    pub triangles: usize,
    /// How many points the file had (more than `points` when the cloud was thinned coming in).
    pub source_points: u64,
    /// The box round the cloud in the design, `[[x, y, z], [x, y, z]]` in mm; null for no points.
    pub bounds: Option<[[f64; 3]; 2]>,
    /// What could not be read from the file, or was thinned out of it.
    pub warnings: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Edit a point cloud: its name, whether it is drawn, the size of its points, how they are
/// coloured and which of them are shown. A key left out keeps what the cloud has; the points
/// themselves change only with `units`, which scales them, and `matrix`, which moves them.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PointCloudEditParams {
    /// The point cloud, by id or name.
    pub cloud: IdOrName,
    /// Its new name. Another cloud may not already have it.
    #[serde(default)]
    pub name: Option<String>,
    /// Whether it is drawn.
    #[serde(default)]
    pub visible: Option<bool>,
    /// The size of a point on screen, in pixels.
    #[serde(default)]
    pub point_size: Option<CloudPointSize>,
    /// How the points are coloured.
    #[serde(default)]
    pub color_by: Option<CloudColorBy>,
    /// The one colour of every point. Given on its own it colours the points with it; `null`
    /// means the file's colours.
    #[serde(default, deserialize_with = "color_or_file")]
    #[schemars(schema_with = "color_schema")]
    pub color: Option<Option<CloudColor>>,
    /// The range the colour ramp runs between. `null` works the range out.
    #[serde(default, deserialize_with = "range_or_auto")]
    #[schemars(schema_with = "range_schema")]
    pub color_range: Option<Option<CloudRange>>,
    /// The same as `color_range`, which is read first when both are given.
    #[serde(default, deserialize_with = "range_or_auto")]
    #[schemars(schema_with = "range_schema")]
    pub height_range: Option<Option<CloudRange>>,
    /// The file's length unit: the points are scaled from the unit they were read as to this one.
    #[serde(default)]
    pub units: Option<CloudUnits>,
    /// Where the cloud sits: a rigid row-major 3×4 or 4×4 matrix, as `occurrence.move` takes it.
    /// `null` leaves the placement as it is.
    #[serde(default)]
    pub matrix: Option<MatrixParam>,
    /// With `matrix`: the largest accepted |RᵀR − I| per entry. Default: 1e-9. Read only with it.
    #[serde(default)]
    pub tolerance: Option<MatrixTolerance>,
    /// Which elements are shown: only what every filter keeps. `null` removes the cloud's filters.
    #[serde(default, deserialize_with = "filters_or_default")]
    #[schemars(with = "CloudFilters")]
    pub filters: Option<Option<CloudFilters>>,
}

/// What `pointcloud.edit` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PointCloudEditResult {
    /// The cloud that was edited, by id.
    pub cloud: u64,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The size of a point on screen, in pixels: a number within the range the design draws at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CloudPointSize(pub f64);

impl<'de> Deserialize<'de> for CloudPointSize {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)?.as_f64() {
            Some(size) if size.is_finite() && POINT_SIZE.contains(&size) => Ok(CloudPointSize(size)),
            _ => Err(D::Error::custom(format!("must be a number of pixels from {} to {}", POINT_SIZE.start(), POINT_SIZE.end()))),
        }
    }
}

impl JsonSchema for CloudPointSize {
    fn schema_name() -> Cow<'static, str> {
        "CloudPointSize".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A point's size on screen, in pixels.",
            "type": "number",
            "minimum": POINT_SIZE.start(),
            "maximum": POINT_SIZE.end()
        })
    }
}

/// How a cloud's points are coloured: the file's colours, one colour, a ramp by height, or a ramp
/// or palette by any property of the file. Any text that is not one of the three words is taken
/// as a property's name, so the schema can only name the three.
#[derive(Clone, Debug, PartialEq)]
pub struct CloudColorBy(pub String);

impl<'de> Deserialize<'de> for CloudColorBy {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(name) => Ok(CloudColorBy(name)),
            _ => Err(D::Error::custom("must be file, single, height or a property's name")),
        }
    }
}

impl JsonSchema for CloudColorBy {
    fn schema_name() -> Cow<'static, str> {
        "CloudColorBy".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "`file` (the file's colours), `single` (one colour), `height` (a ramp by height, blue low to red high) or a property's name (a ramp over its values, or a palette for whole numbers). Any other text is a property's name; the cloud must have it.",
            "type": "string"
        })
    }
}

/// One colour for every point: `[r, g, b]`, each from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CloudColor(pub [f32; 3]);

impl<'de> Deserialize<'de> for CloudColor {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Array(a) if a.len() == 3 => {
                let color: Option<Vec<f32>> = a.iter().map(|v| v.as_f64().filter(|x| (0.0..=1.0).contains(x)).map(|x| x as f32)).collect();
                match color.as_deref() {
                    Some(&[r, g, b]) => Ok(CloudColor([r, g, b])),
                    _ => Err(D::Error::custom("must be [r, g, b] with each from 0 to 1")),
                }
            }
            _ => Err(D::Error::custom("must be [r, g, b] with each from 0 to 1, or null")),
        }
    }
}

impl JsonSchema for CloudColor {
    fn schema_name() -> Cow<'static, str> {
        "CloudColor".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A colour as [r, g, b], each from 0 to 1.",
            "type": "array",
            "items": {"type": "number", "minimum": 0, "maximum": 1},
            "minItems": 3,
            "maxItems": 3
        })
    }
}

/// `color` as the handler reads it: absent, `null` for the file's colours, or a colour.
fn color_or_file<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<CloudColor>>, D::Error> {
    Option::<CloudColor>::deserialize(d).map(Some)
}

fn color_schema(generator: &mut SchemaGenerator) -> Schema {
    let color = generator.subschema_for::<CloudColor>();
    json_schema!({
        "description": "One colour for every point, `[r, g, b]` with each from 0 to 1. `null` means the file's colours.",
        "anyOf": [color, {"type": "null"}]
    })
}

/// The range a colour ramp runs between: `[low, high]`, lowest first, both within the design's
/// coordinates. The handler's message names both keys, so it is given whole.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CloudRange(pub [f64; 2]);

impl<'de> Deserialize<'de> for CloudRange {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let refuse = || D::Error::custom(whole("`height_range` (or `color_range`) must be [low, high], lowest first, or null"));
        match Value::deserialize(d)? {
            Value::Array(a) => match a.iter().map(Value::as_f64).collect::<Option<Vec<f64>>>().as_deref() {
                Some(&[low, high]) if low.is_finite() && high.is_finite() && low < high && low.abs() <= MAX_COORD && high.abs() <= MAX_COORD => {
                    Ok(CloudRange([low, high]))
                }
                _ => Err(refuse()),
            },
            _ => Err(refuse()),
        }
    }
}

impl JsonSchema for CloudRange {
    fn schema_name() -> Cow<'static, str> {
        "CloudRange".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A range as [low, high], lowest first, each end a finite coordinate within 1e9.",
            "type": "array",
            "items": {"type": "number"},
            "minItems": 2,
            "maxItems": 2
        })
    }
}

/// `color_range` and `height_range` as the handler reads them: absent, `null` for the automatic
/// range, or a range.
fn range_or_auto<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<CloudRange>>, D::Error> {
    Option::<CloudRange>::deserialize(d).map(Some)
}

fn range_schema(generator: &mut SchemaGenerator) -> Schema {
    let range = generator.subschema_for::<CloudRange>();
    json_schema!({
        "description": "The range the colour ramp runs between, `[low, high]` lowest first: design Z in mm by height, the property's values by a property. `null` works it out: all but the lowest and highest 2 % of the points by height, or the values the property takes over what the filters keep.",
        "anyOf": [range, {"type": "null"}]
    })
}

/// Which elements of a cloud are shown: only what every filter keeps. Read by the handler's own
/// reader, so a filter means exactly what it means there.
#[derive(Clone, Debug, PartialEq)]
pub struct CloudFilters(pub Vec<Filter>);

impl<'de> Deserialize<'de> for CloudFilters {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut given = Map::new();
        given.insert("filters".to_string(), Value::deserialize(d)?);
        // The key is always there, so the reader never answers `None`; a cloud with no filters is
        // an empty list, which is what `null` means.
        match crate::cmd::point_cloud_filters(&Value::Object(given), "") {
            Ok(Some(keep)) => Ok(CloudFilters(keep)),
            Ok(None) => Ok(CloudFilters(Vec::new())),
            Err(e) => Err(D::Error::custom(whole(refusal(e)))),
        }
    }
}

/// `filters` as the handlers read it: absent (`None`), `null` for none (`Some(None)`), or a list
/// of filters (`Some(Some(_))`).
fn filters_or_default<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<CloudFilters>>, D::Error> {
    Option::<CloudFilters>::deserialize(d).map(Some)
}

impl JsonSchema for CloudFilters {
    fn schema_name() -> Cow<'static, str> {
        "CloudFilters".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let filter = generator.subschema_for::<PointCloudFilter>();
        json_schema!({
            "description": format!(
                "At most {MAX_FILTERS} filters, each {{property, range: [low, high]}} (both ends included) or {{property, values: [whole numbers]}} for a property with named values, at most {MAX_NAMES} of them. The cloud must have the property. `null` means no filters."
            ),
            "anyOf": [
                {"type": "array", "items": filter, "maxItems": MAX_FILTERS},
                {"type": "null"}
            ]
        })
    }
}

/// Read a point cloud's file again from where it was inserted. The points, colours, segments,
/// triangles and properties are replaced; the name, placement, units, point size and colouring
/// are kept, and so are the filters on properties the file still has. The file is read under the
/// limit the cloud was inserted with, so a file that grew is thinned only past that limit.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PointCloudReloadParams {
    /// The point cloud, by id or name.
    pub cloud: IdOrName,
}

/// What `pointcloud.reload` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PointCloudReloadResult {
    /// The cloud's id.
    pub cloud: u64,
    /// How many points it has now.
    pub points: usize,
    /// How many segments it has now.
    pub segments: usize,
    /// How many triangles it has now.
    pub triangles: usize,
    /// What could not be read from the file, or was thinned out of it.
    pub warnings: Vec<String>,
}

/// Delete a point cloud. Its file is left where it is; only the design forgets the cloud.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PointCloudDeleteParams {
    /// The point cloud, by id or name.
    pub cloud: IdOrName,
}

/// What `pointcloud.delete` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PointCloudDeleteResult {
    /// The cloud that was deleted, by id.
    pub deleted: u64,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `pointcloud.list` takes no parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PointCloudListParams {}

/// What `pointcloud.list` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PointCloudListResult {
    /// Every point cloud in the design.
    pub clouds: Vec<PointCloudInfo>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One point cloud, as the design keeps it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PointCloudInfo {
    /// The cloud's id.
    pub cloud: u64,
    /// Its name in the design.
    pub name: String,
    /// The file it was read from (its name only).
    pub file: String,
    /// The path `pointcloud.reload` reads it again from; empty when the file was not kept.
    pub source: String,
    /// How many points it has now.
    pub points: usize,
    /// How many segments it has.
    pub segments: usize,
    /// How many triangles it has.
    pub triangles: usize,
    /// How many points the file had (more than `points` when the cloud was thinned coming in).
    pub source_points: u64,
    /// The file's length unit: `mm`, `cm`, `m`, `in` or `ft`.
    pub units: String,
    /// Whether it is drawn.
    pub visible: bool,
    /// Size of a point on screen (pixels).
    pub point_size: f64,
    /// How the points are coloured: `file`, `single`, `height` or a property's name.
    pub color_by: String,
    /// The one colour of every point, with `color_by: single`; null means the plain one.
    pub color: Option<[f32; 3]>,
    /// Whether the file carries a colour of each point.
    pub colored: bool,
    /// The properties the file carries, the point coordinates first.
    pub properties: Vec<PointCloudProperty>,
    /// The filters on its properties: only what every filter keeps is shown.
    pub filters: Vec<PointCloudFilter>,
    /// The colour ramp's range now, exact over what the filters keep; null when it is not a ramp.
    pub color_range: Option<[f64; 2]>,
    /// The ramp's own range, or the automatic one by height; null when it is worked out.
    pub height_range: Option<[f64; 2]>,
    /// Whether the ramp's range is worked out from the points.
    pub height_auto: bool,
    /// The box round the cloud in the design, `[[x, y, z], [x, y, z]]` in mm; null for no points.
    pub bounds: Option<[[f64; 3]; 2]>,
    /// Where the cloud sits: a rigid row-major 3×4 matrix, 12 numbers, row by row.
    pub matrix: Vec<f64>,
}

/// One property a point cloud's file carries.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PointCloudProperty {
    /// Its name; `x`, `y` and `z` are the coordinates of each point in the design.
    pub name: String,
    /// Whether its values are whole numbers, which a colour ramp takes as a palette.
    pub integer: bool,
    /// Its smallest and largest value, the coordinates in the design; null when it has no values.
    pub span: Option<[f64; 2]>,
    /// The names of whole-number values, `[number, name]`, as the file gives them; null when it
    /// gives none.
    pub names: Option<Vec<(i32, String)>>,
}

/// A filter on one of a point cloud's properties.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PointCloudFilter {
    /// The property it filters.
    pub property: String,
    #[serde(flatten)]
    pub keep: PointCloudFilterKeeps,
}

/// The values a filter keeps.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum PointCloudFilterKeeps {
    /// Whose value lies in the range, both ends included.
    Range { range: [f64; 2] },
    /// Which are one of these whole numbers, for a property with named values.
    Values { values: Vec<i32> },
}

#[cfg(test)]
#[path = "point_cloud_tests.rs"]
mod tests;
