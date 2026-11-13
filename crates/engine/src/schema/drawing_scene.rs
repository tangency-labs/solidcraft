//! Contracts and result types for `drawing.scene` (`cmd/drawing_scene.rs`, #4890).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};
use solidcraft_doc::ids;
use solidcraft_geom::paper::ObjectKind;

use super::Point2;
use super::drawing_sheet::sheet_ref;
use super::drawing_view::{BoxResult, LineSourceResult, LinesStatsResult, paper_scene_schema};
use crate::drawing_scene::{SheetObject, ViewState};

/// Draw one sheet of the open drawing. Read-only.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SheetSceneParams {
    /// The sheet, by id or name.
    #[serde(deserialize_with = "sheet_ref")]
    pub sheet: String,
}

/// A sheet's paper scene: its size and its groups in paint order, in paper millimetres from the
/// sheet's lower-left corner: the drawing border's (object: the sheet), then each image's picture
/// (object and source: the placement), then each view's strokes (object: the view), then each
/// dimension's lines and arrowheads (object: the dimension) and each note of a view (object: the
/// note), then each note on the sheet, then the title block's (object: the sheet).
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct SheetPaperScene(pub solidcraft_geom::paper::Scene);

impl JsonSchema for SheetPaperScene {
    fn schema_name() -> Cow<'static, str> {
        "SheetPaperScene".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        paper_scene_schema(
            generator,
            &[ObjectKind::DrawingBorder, ObjectKind::SheetImage, ObjectKind::View, ObjectKind::Dimension, ObjectKind::Note, ObjectKind::TitleBlock],
            true,
        )
    }
}

/// One view on the sheet: `drawn`, `placeholder` (nothing to measure, so no lines) or `failed`,
/// with why. A drawn view can say why too, as a note does: its position names joints that are gone.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SheetSceneView {
    pub view: String,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// What working out its lines cost, when they were.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<LinesStatsResult>,
}

impl From<ViewState> for SheetSceneView {
    fn from(v: ViewState) -> Self {
        SheetSceneView { view: ids::encode(v.view), status: v.status.name(), reason: v.status.reason().or(v.why), stats: v.stats.map(Into::into) }
    }
}

/// One dimension of a view on the sheet: where it stands with the model, in
/// `drawing.dimension.inspect`'s words, and whether its lines are in the scene, with why not.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SheetSceneDimension {
    pub dimension: String,
    pub view: String,
    pub status: &'static str,
    pub drawn: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// One note of the sheet: on a view (or what it shows) or on the sheet itself; where what it points
/// at stands with the model, in `drawing.dimension.inspect`'s words (ok when it points at nothing);
/// and whether it is in the scene, with why not, or what of it is left out.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SheetSceneNote {
    pub note: String,
    /// The view it moves with; none for a note on the sheet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    pub status: &'static str,
    pub drawn: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// Where one object of the scene sits on paper: the box round what it draws, along its lines' paths
/// (with no lines, its boundary), and a view's boundary.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SheetObjectResult {
    pub object: String,
    #[schemars(with = "String")]
    pub kind: ObjectKind,
    pub bounds_mm: BoxResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boundary_mm: Option<BoxResult>,
}

impl From<SheetObject> for SheetObjectResult {
    fn from(o: SheetObject) -> Self {
        let on_paper = |b: solidcraft_geom::Aabb2| BoxResult { min: b.min.into(), max: b.max.into() };
        SheetObjectResult { object: ids::encode(o.object), kind: o.kind, bounds_mm: on_paper(o.bounds_mm), boundary_mm: o.boundary_mm.map(on_paper) }
    }
}

/// What is under a point of one sheet of the open drawing. Read-only.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct SheetHitParams {
    /// The sheet, by id or name.
    #[serde(deserialize_with = "sheet_ref")]
    pub sheet: String,
    /// The point, in paper millimetres from the sheet's lower-left corner.
    pub point_mm: Point2,
    /// How near a box's edge counts as on it, in paper millimetres: 0 or more (0 when not given).
    #[serde(default)]
    pub band_mm: f64,
}

/// The object a point hit, with its kind as `drawing.scene`'s objects give it, and whether the point
/// is on its box's edge.
#[derive(Debug, Serialize, JsonSchema)]
pub struct HitResult {
    pub object: String,
    #[schemars(with = "String")]
    pub kind: ObjectKind,
    pub on_edge: bool,
}

/// What `drawing.scene.hit` returns: the object, or null when the point hits nothing.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SheetHitResult {
    pub hit: Option<HitResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The line under a point of one sheet of the open drawing. Read-only.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct SheetLineParams {
    /// The sheet, by id or name.
    #[serde(deserialize_with = "sheet_ref")]
    pub sheet: String,
    /// The point, in paper millimetres from the sheet's lower-left corner.
    pub point_mm: Point2,
    /// How far from a line the point may be and still name it, in paper millimetres: 0 or more (1
    /// when not given).
    #[serde(default = "one_mm")]
    pub band_mm: f64,
}

fn one_mm() -> f64 {
    1.0
}

/// The line a point names: its view, what drew it in `drawing.view.snapshot`'s words, and how far
/// the point is from it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LineHitResult {
    pub view: String,
    /// The occurrences that place its body, from the design's root.
    pub occurrence: Vec<u64>,
    /// Its body's name in its definition.
    pub body: String,
    /// Whether the body is a reference part.
    pub reference: bool,
    pub source: LineSourceResult,
    pub distance_mm: f64,
}

/// What `drawing.scene.line` returns: the line, or null when the point names none.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SheetLineResult {
    pub line: Option<LineHitResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Where the drawing open stands. It takes nothing.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct DrawingStatusParams {}

/// Lets the drawing open draw from the model as it is now. It takes nothing.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct DrawingUpdateParams {}

/// What `drawing.update` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DrawingUpdateResult {
    /// How many views wait to be drawn: the sheet page draws them, as a command that reads their sheet does.
    pub waiting: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One view: `current`, `out_of_date`, `placeholder` or `failed`, with why for the last two.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ViewStatusResult {
    pub view: String,
    pub sheet: String,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// One dimension, in the words `drawing.dimension.inspect` uses.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DimensionStatusResult {
    pub dimension: String,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// Its sheet shows it as it last was, or not at all: its view is out of date, or it was read from
    /// another model or record than it has now.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub out_of_date: bool,
}

/// What `drawing.status` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DrawingStatusResult {
    /// Each view of the drawing, in id order.
    pub views: Vec<ViewStatusResult>,
    /// Each dimension, in id order.
    pub dimensions: Vec<DimensionStatusResult>,
    /// How many views are out of date.
    pub waiting: usize,
    /// A deferred drawing: its views and dimensions out of date wait for `drawing.update`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub held: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `drawing.scene` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SheetSceneResult {
    pub sheet: String,
    pub scene: SheetPaperScene,
    /// Each view on the sheet, in id order.
    pub views: Vec<SheetSceneView>,
    /// The sheet's border, then each view on it, then each dimension and note drawn, in paint order:
    /// each that is on paper. What draws nothing and has no boundary isn't one.
    pub objects: Vec<SheetObjectResult>,
    /// Each dimension of a view on the sheet, by view and id.
    pub dimensions: Vec<SheetSceneDimension>,
    /// Each note of a view on the sheet, by view and id, then each note on the sheet, by id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<SheetSceneNote>,
    /// The views, dimensions and notes a deferred drawing shows as they last were.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub out_of_date: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
