//! `drawing.export` (#4890 A7): one sheet of the open drawing written out from its scene
//! ([`crate::drawing_scene::build`]), the scene the sheet page draws, so a file shows what the page
//! does. SVG for now: PNG comes with the sheet PNG writer (#4890 A4) and DXF with the shared DXF
//! writer (#5319). A sheet with a view that failed, an image that won't decode or a picture that
//! can't be drawn isn't written; the refusal names them.

use std::path::Path;

use serde_json::Value;
use solidcraft_doc::drawing::DEFAULT_DPI;
use solidcraft_doc::ids;

use super::CommandSpec;
use crate::drawing_pictures::SheetPictures;
use crate::drawing_scene::{Status, sheet_id};
use crate::params::bad;
use crate::schema::drawing_export::{ExportFormat, ExportParams, ExportReport, ExportResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const CMD: &str = "drawing.export";
const CONTRACT: TypedContract<ExportParams> = TypedContract::new();
/// The longest path taken, in bytes, as the view DXF's.
const MAX_PATH_BYTES: usize = 4096;

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(CMD, "Export Sheet", crate::checked!("drawing.export", export))
    .icon("export")
    .in_drawing()
    .noundo()
    .doc_only()
    .contract(&CONTRACT)
    .output(output_schema_for::<ExportResult>)
    .params("sheet: id or name; format?: svg (default; png and dxf are not supported yet); path?: .svg file (else the reply holds the SVG text) → {sheet, format, bytes, path | svg, report: {widths_snapped, pieces_merged}, views: [{view, status: drawn|placeholder|failed, reason?, stats?}]}. The sheet as drawing.scene draws it: its drawing border, each image on it, then each view's visible lines. Refused, naming them, while any view on the sheet failed or any image won't decode; a placeholder view draws nothing. No model is evaluated.")];

fn export(s: &mut Session, p: &Value) -> Result<Value> {
    let request: ExportParams = crate::schema::read(CMD, p)?;
    match request.format {
        ExportFormat::Svg => {}
        ExportFormat::Png => return Err(bad(CMD, "not supported yet: PNG needs the sheet PNG writer (#4890 A4)")),
        ExportFormat::Dxf => return Err(bad(CMD, "not supported yet: DXF needs the shared DXF writer (#5319)")),
    }
    if let Some(path) = &request.path {
        let svg = Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("svg"));
        if path.len() > MAX_PATH_BYTES || !svg {
            return Err(bad(CMD, "path must be a .svg file"));
        }
        solidcraft_io::vfs::check(path).map_err(|e| EngineError::Other(e.to_string()))?;
    }
    let d = s.drawing(CMD)?.clone();
    let sheet = sheet_id(&d, &request.sheet, CMD)?;
    let built = crate::drawing_scene::build(s, &d, sheet)?;
    let failed: Vec<String> = built
        .views
        .iter()
        .filter_map(|v| match &v.status {
            Status::Failed(why) => Some(format!("{} ({why})", ids::encode(v.view))),
            _ => None,
        })
        .collect();
    if !failed.is_empty() {
        return Err(bad(CMD, format!("not written: views on the sheet failed: {}", failed.join(", "))));
    }
    let prepared = built.scene.prepare().map_err(|e| bad(CMD, e.to_string()))?;
    let pictures = SheetPictures::new(s, &d, sheet).map_err(|e| bad(CMD, e))?;
    let (svg, written) = solidcraft_io::paper::paper_svg(&prepared, &pictures, DEFAULT_DPI).map_err(|e| bad(CMD, e.to_string()))?;
    let bytes = svg.len();
    let (path, svg) = match request.path {
        Some(path) => {
            // Checked again just before writing, as the view DXF does.
            solidcraft_io::vfs::check(&path).map_err(|e| EngineError::Other(e.to_string()))?;
            solidcraft_io::write_atomic(Path::new(&path), &svg, false).map_err(|e| EngineError::Other(e.to_string()))?;
            (Some(path), None)
        }
        None => (None, Some(String::from_utf8(svg).map_err(|e| EngineError::Other(e.to_string()))?)),
    };
    let reply = ExportResult {
        sheet: ids::encode(sheet),
        format: request.format,
        bytes,
        path,
        svg,
        report: ExportReport { widths_snapped: written.widths_snapped, pieces_merged: written.pieces_merged },
        views: built.views.into_iter().map(Into::into).collect(),
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

#[cfg(test)]
#[path = "drawing_export_tests.rs"]
mod tests;
