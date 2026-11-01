//! Contracts and result types for `drawing.scene` (`cmd/drawing_scene.rs`, #4890).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};
use solidcraft_doc::ids;
use solidcraft_geom::paper::ObjectKind;

use super::drawing_sheet::sheet_ref;
use super::drawing_view::{LinesStatsResult, paper_scene_schema};
use crate::drawing_scene::{Status, ViewState};

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
/// (object and source: the placement), then each view's strokes (object: the view), then the title
/// block's (object: the sheet).
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct SheetPaperScene(pub solidcraft_geom::paper::Scene);

impl JsonSchema for SheetPaperScene {
    fn schema_name() -> Cow<'static, str> {
        "SheetPaperScene".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        paper_scene_schema(generator, &[ObjectKind::DrawingBorder, ObjectKind::SheetImage, ObjectKind::View, ObjectKind::TitleBlock], true)
    }
}

/// One view on the sheet: `drawn`, `placeholder` (nothing to measure, so no lines) or `failed`,
/// with why.
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
        let status = v.status.name();
        let reason = match v.status {
            Status::Drawn => None,
            Status::Placeholder(why) => Some(why.to_string()),
            Status::Failed(why) => Some(why),
        };
        SheetSceneView { view: ids::encode(v.view), status, reason, stats: v.stats.map(Into::into) }
    }
}

/// What `drawing.scene` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SheetSceneResult {
    pub sheet: String,
    pub scene: SheetPaperScene,
    /// Each view on the sheet, in id order.
    pub views: Vec<SheetSceneView>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
