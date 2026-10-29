//! `drawing.scene` (#4890 W10): one sheet of the open drawing as one paper scene, drawn at once
//! ([`crate::drawing_scene::build`]): its drawing border, then each view on it by id. The sheet
//! page draws the same scene through the provider, and the drawing export (A7) writes it.

use serde_json::Value;
use solidcraft_doc::ids;

use super::CommandSpec;
use crate::drawing_scene::CMD;
use crate::schema::drawing_scene::{SheetPaperScene, SheetSceneParams, SheetSceneResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const CONTRACT: TypedContract<SheetSceneParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(CMD, "Draw Sheet", crate::checked!("drawing.scene", CONTRACT, scene))
    .icon("export")
    .in_drawing()
    .noundo()
    .doc_only()
    .contract(&CONTRACT)
    .output(output_schema_for::<SheetSceneResult>)
    .params("sheet: id or name → {sheet, scene: {size_mm, groups: [{object, kind: drawing_border|view, items: [{kind: stroke, path, closed, width_mm} or {kind: fill, region}]}]} in paint order: the sheet's drawing border (object: the sheet), then each view on it by id (object: the view; its visible lines only for now), views: [{view, status: drawn|placeholder|failed, reason?, stats?}]}. A view with nothing to measure, or one that can't be drawn yet, is left out and says why. Read-only; no model is evaluated.")];

fn scene(s: &mut Session, p: &Value) -> Result<Value> {
    let request: SheetSceneParams = crate::schema::read(CMD, p)?;
    let d = s.drawing(CMD)?.clone();
    let sheet = crate::drawing_scene::sheet_id(&d, &request.sheet, CMD)?;
    let built = crate::drawing_scene::build(s, &d, sheet)?;
    let views = built.views.into_iter().map(Into::into).collect();
    let reply = SheetSceneResult { sheet: ids::encode(sheet), scene: SheetPaperScene(built.scene), views, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}
