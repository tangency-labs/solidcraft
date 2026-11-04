//! `drawing.scene` (#4890 W10): one sheet of the open drawing as one paper scene, drawn at once
//! ([`crate::drawing_scene::build`]): its drawing border, each image on it, then each view on it by id, then
//! its title block. The sheet page draws the same scene through the provider, and the drawing export
//! (A7) writes it.
//!
//! `drawing.status` (#4894 WE2) says where the drawing open stands: each view as the provider last
//! drew it, and each dimension as the model has it now, in one read.

use serde_json::Value;
use solidcraft_doc::ids;

use super::CommandSpec;
use crate::drawing_scene::{CMD, DimensionState};
use crate::schema::drawing_scene::{
    DimensionStatusResult, DrawingStatusParams, DrawingStatusResult, SheetPaperScene, SheetSceneDimension, SheetSceneParams, SheetSceneResult,
    ViewStatusResult,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const CONTRACT: TypedContract<SheetSceneParams> = TypedContract::new();
const STATUS: &str = "drawing.status";
const STATUS_CONTRACT: TypedContract<DrawingStatusParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(STATUS, "Drawing Status", crate::checked!("drawing.status", status))
    .icon("inspect")
    .in_drawing()
    .noundo()
    .doc_only()
    .contract(&STATUS_CONTRACT)
    .output(output_schema_for::<DrawingStatusResult>)
    .params("→ {views: [{view, sheet, status: current|out_of_date|placeholder|failed, reason?}], dimensions: [{dimension, status: ok|overridden|ambiguous|suppressed|lost|unmeasurable, why?}], waiting: how many views are out of date}. current: drawn, from the model and with the settings the view has now. Balloons aren't listed: drawing.balloon.inspect reads each one. Read-only: nothing is drawn, and each body a dimension names is read once for the call."),
    CommandSpec::new(CMD, "Draw Sheet", crate::checked!("drawing.scene", scene))
    .icon("export")
    .in_drawing()
    .noundo()
    .doc_only()
    .contract(&CONTRACT)
    .output(output_schema_for::<SheetSceneResult>)
    .params("sheet: id or name → {sheet, scene: {size_mm, groups: [{object, kind: drawing_border|sheet_image|view|dimension|title_block, items: [{kind: stroke, path, closed, width_mm} or {kind: fill, region} or {kind: picture, source, rect: {min, max}, opacity}]}]} in paint order: the sheet's drawing border (object: the sheet), then each image on it by id (object and source: the placement), then each view on it by id (object: the view; its visible lines only for now), then the dimensions of those views by view and id (object: the dimension; lines and arrowheads, in paper millimetres at any scale, with no text yet), then its title block's lines (object: the sheet), views: [{view, status: drawn|placeholder|failed, reason?, stats?}], objects: [{object, kind, bounds_mm: {min, max}, boundary_mm?}], where the border, each view and each dimension sit on paper, dimensions: [{dimension, view, status: ok|overridden|ambiguous|suppressed|lost|unmeasurable, drawn, why?}]}. A view with nothing to measure, or one that can't be drawn yet, is left out and says why, and so is a dimension that isn't found again or whose view isn't drawn. Read-only; no model is evaluated.")];

fn status(s: &mut Session, p: &Value) -> Result<Value> {
    let _: DrawingStatusParams = crate::schema::read(STATUS, p)?;
    let d = s.drawing(STATUS)?.clone();
    let views: Vec<ViewStatusResult> = crate::drawing_scene::standing(s, &d)
        .into_iter()
        .map(|v| ViewStatusResult { view: ids::encode(v.view), sheet: ids::encode(v.sheet), status: v.status, reason: v.reason })
        .collect();
    let wanted: Vec<u64> = d.dimensions.iter().map(|dimension| dimension.id).collect();
    let read = s.doc.resolve_dimensions(&s.model, &d, &wanted).map_err(|e| crate::params::bad(STATUS, e.to_string()))?;
    let dimensions = wanted
        .iter()
        .zip(read)
        .map(|(id, r)| {
            let (status, why, _) = super::drawing_dimension::status_words(r.status);
            DimensionStatusResult { dimension: ids::encode(*id), status, why }
        })
        .collect();
    let waiting = views.iter().filter(|v| v.status == "out_of_date").count();
    let reply = DrawingStatusResult { views, dimensions, waiting, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// Each dimension of a scene in the words of a reply.
pub(super) fn dimension_results(states: Vec<DimensionState>) -> Vec<SheetSceneDimension> {
    let one = |state: DimensionState| {
        let (status, why, _) = super::drawing_dimension::status_words(state.status);
        SheetSceneDimension {
            dimension: ids::encode(state.dimension),
            view: ids::encode(state.view),
            status,
            drawn: state.drawn,
            why: why.or(state.why),
        }
    };
    states.into_iter().map(one).collect()
}

fn scene(s: &mut Session, p: &Value) -> Result<Value> {
    let request: SheetSceneParams = crate::schema::read(CMD, p)?;
    let d = s.drawing(CMD)?.clone();
    let sheet = crate::drawing_scene::sheet_id(&d, &request.sheet, CMD)?;
    let built = crate::drawing_scene::build(s, &d, sheet)?;
    let views = built.views.into_iter().map(Into::into).collect();
    let objects = built.objects.into_iter().map(Into::into).collect();
    let dimensions = dimension_results(built.dimensions);
    let reply =
        SheetSceneResult { sheet: ids::encode(sheet), scene: SheetPaperScene(built.scene), views, objects, dimensions, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}
