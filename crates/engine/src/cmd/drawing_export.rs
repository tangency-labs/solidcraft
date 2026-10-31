//! `drawing.export` (#4890 A7): one sheet of the open drawing written out from its scene
//! ([`crate::drawing_scene::build`]), the scene the sheet page draws, so a file shows what the page
//! does: SVG, or PNG at a chosen dpi and colour. DXF comes with the shared DXF writer (#5319). A
//! sheet with a view that failed isn't written; the refusal names the views. Without a path the
//! reply holds the document, as text or as base64. A large PNG can be written as a job
//! ([`crate::drawing_export`]), a band at a time.

use std::path::Path;

use serde_json::Value;
use solidcraft_doc::canvas::base64_encode;
use solidcraft_doc::ids;
use solidcraft_geom::paper::Prepared;
use solidcraft_io::paper::{DPI, PngOptions, PngWriter};
use solidcraft_io::{AtomicFile, MAX_DRAWING_BYTES};

use super::CommandSpec;
use crate::drawing_scene::{Status, ViewState, sheet_id};
use crate::params::bad;
use crate::schema::drawing_export::{ExportFormat, ExportJobParams, ExportJobStatus, ExportParams, ExportReply, ExportReport, ExportResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const CMD: &str = "drawing.export";
const CONTRACT: TypedContract<ExportParams> = TypedContract::new();
const JOB: TypedContract<ExportJobParams> = TypedContract::new();
/// The longest path taken, in bytes, as the view DXF's.
const MAX_PATH_BYTES: usize = 4096;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new(CMD, "Export Sheet", crate::checked!("drawing.export", export))
        .icon("export")
        .in_drawing()
        .noundo()
        .doc_only()
        .contract(&CONTRACT)
        .output(output_schema_for::<ExportReply>)
        .params("sheet: id or name; format?: svg (default) | png (dxf is not supported yet); path?: a file with the format's extension (else the reply holds the document: text for svg, base64 data for png, at most 64 MiB); png only: dpi?: 25 to 2400 (default the sheet's own), colour?: grey (default) | black_white | rgb | transparent; job?: false, png to a path only: written a band at a time as the app draws, answered at once as drawing.export.status answers → {sheet, format, bytes, path | text | data, report: {widths_snapped, pieces_merged, pixels?: [width, height]}, views: [{view, status: drawn|placeholder|failed, reason?, stats?}]}. The sheet as drawing.scene draws it: its drawing border, then each view's visible lines. Refused, naming them, while any view on the sheet failed; a placeholder view draws nothing. No model is evaluated."),
    CommandSpec::new("drawing.export.status", "Sheet Export Progress", crate::checked!("drawing.export.status", status))
        .icon("list")
        .any_part()
        .noundo()
        .doc_only()
        .contract(&JOB)
        .output(output_schema_for::<ExportJobStatus>)
        .params("job: the number drawing.export gave a job → {job, progress: {phase: running|completed|cancelled|failed, done, total} in rows of pixels, error: {message} | null, result?: drawing.export's reply once completed}. Only the session's latest job answers. Writes nothing itself: the app writes a band each time it draws."),
    CommandSpec::new("drawing.export.cancel", "Cancel Sheet Export", crate::checked!("drawing.export.cancel", cancel))
        .icon("delete")
        .any_part()
        .noundo()
        .doc_only()
        .contract(&JOB)
        .output(output_schema_for::<ExportJobStatus>)
        .params("job → its status, as drawing.export.status. A job under way stops: its file isn't written and a file already at the path stays as it was. An ended job is answered as it ended."),
];

/// A document held for the reply, refused once it passes `limit` bytes: a larger one goes to a file.
struct Held {
    bytes: Vec<u8>,
    limit: usize,
}

impl std::io::Write for Held {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len().saturating_add(buf.len()) > self.limit {
            return Err(std::io::Error::other(format!("over {} MiB for the reply: give a path", self.limit >> 20)));
        }
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn export(s: &mut Session, p: &Value) -> Result<Value> {
    let request: ExportParams = crate::schema::read(CMD, p)?;
    let d = s.drawing(CMD)?.clone();
    let sheet = sheet_id(&d, &request.sheet, CMD)?;
    let png = match request.format {
        ExportFormat::Svg => None,
        ExportFormat::Png => {
            // The sheet's own resolution unless the call gives one.
            let own = d.sheets.iter().find(|s| s.id == sheet).map(|s| s.dpi);
            let dpi = request.dpi.or(own).unwrap_or(PngOptions::default().dpi);
            if !DPI.contains(&dpi) {
                return Err(bad(CMD, "`dpi` is 25 to 2400"));
            }
            Some(PngOptions { dpi, colour: request.colour.unwrap_or_default().into() })
        }
        ExportFormat::Dxf => return Err(bad(CMD, "not supported yet: DXF needs the shared DXF writer (#5319)")),
    };
    if png.is_none() && (request.dpi.is_some() || request.colour.is_some()) {
        return Err(bad(CMD, "`dpi` and `colour` are for png only"));
    }
    if request.job && (png.is_none() || request.path.is_none()) {
        return Err(bad(CMD, "`job` is for a png written to a path"));
    }
    if let (true, Some(job)) = (request.job, s.drawing_exports.running()) {
        return Err(bad(CMD, format!("an export is under way (job {job}): cancel it or wait for it")));
    }
    let extension = request.format.extension();
    if let Some(path) = &request.path {
        let named = Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case(extension));
        if path.len() > MAX_PATH_BYTES || !named {
            return Err(bad(CMD, format!("path must be a .{extension} file")));
        }
        solidcraft_io::vfs::check(path).map_err(|e| EngineError::Other(e.to_string()))?;
    }
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
    if let (true, Some(options), Some(path)) = (request.job, png, &request.path) {
        return start_job(s, &prepared, options, path, sheet, built.views);
    }
    let (document, grid, pixels) = match png {
        None => {
            let (svg, grid) = solidcraft_io::paper::paper_svg(&prepared).map_err(|e| bad(CMD, e.to_string()))?;
            (svg, grid, None)
        }
        Some(options) => {
            // Bounded only for the reply: a file may be as large as the sheet needs.
            let limit = if request.path.is_some() { usize::MAX } else { MAX_DRAWING_BYTES };
            let mut held = Held { bytes: Vec::new(), limit };
            let written = solidcraft_io::paper::paper_png(&prepared, options, &mut held).map_err(|e| bad(CMD, e.to_string()))?;
            (held.bytes, written.grid, Some(written.pixels))
        }
    };
    let bytes = document.len();
    let (path, text, data) = match request.path {
        Some(path) => {
            // Checked again just before writing, as the view DXF does.
            solidcraft_io::vfs::check(&path).map_err(|e| EngineError::Other(e.to_string()))?;
            solidcraft_io::write_atomic(Path::new(&path), &document, false).map_err(|e| EngineError::Other(e.to_string()))?;
            (Some(path), None, None)
        }
        None if png.is_some() => (None, None, Some(base64_encode(&document))),
        None => (None, Some(String::from_utf8(document).map_err(|e| EngineError::Other(e.to_string()))?), None),
    };
    let reply = ExportResult {
        sheet: ids::encode(sheet),
        format: request.format,
        bytes,
        path,
        text,
        data,
        report: ExportReport { widths_snapped: grid.widths_snapped, pieces_merged: grid.pieces_merged, pixels },
        views: built.views.into_iter().map(Into::into).collect(),
        param_warnings: Vec::new(),
    };
    serde_json::to_value(ExportReply::Written(reply)).map_err(|e| EngineError::Other(e.to_string()))
}

/// The sheet's PNG started as a job ([`crate::drawing_export`]), answered at once with its status.
fn start_job(s: &mut Session, prepared: &Prepared, options: PngOptions, path: &str, sheet: u64, views: Vec<ViewState>) -> Result<Value> {
    let png = PngWriter::new(prepared, options).map_err(|e| bad(CMD, e.to_string()))?;
    // Checked again just before the file is made; the job checks it once more before putting it in place.
    solidcraft_io::vfs::check(path).map_err(|e| EngineError::Other(e.to_string()))?;
    let file = AtomicFile::create(Path::new(path)).map_err(|e| EngineError::Other(e.to_string()))?;
    let report = png.report();
    let reply = ExportResult {
        sheet: ids::encode(sheet),
        format: ExportFormat::Png,
        bytes: 0,
        path: Some(path.to_owned()),
        text: None,
        data: None,
        report: ExportReport { widths_snapped: report.grid.widths_snapped, pieces_merged: report.grid.pieces_merged, pixels: Some(report.pixels) },
        views: views.into_iter().map(Into::into).collect(),
        param_warnings: Vec::new(),
    };
    let result = serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))?;
    let status = s.drawing_exports.start(path.to_owned(), png, file, result);
    serde_json::to_value(ExportReply::Job(status)).map_err(|e| EngineError::Other(e.to_string()))
}

fn status(s: &mut Session, p: &Value) -> Result<Value> {
    let request: ExportJobParams = crate::schema::read("drawing.export.status", p)?;
    let status = s.drawing_exports.status(request.job).ok_or_else(|| bad("drawing.export.status", format!("no export job {}", request.job)))?;
    serde_json::to_value(status).map_err(|e| EngineError::Other(e.to_string()))
}

fn cancel(s: &mut Session, p: &Value) -> Result<Value> {
    let request: ExportJobParams = crate::schema::read("drawing.export.cancel", p)?;
    let status = s.drawing_exports.cancel(request.job).ok_or_else(|| bad("drawing.export.cancel", format!("no export job {}", request.job)))?;
    serde_json::to_value(status).map_err(|e| EngineError::Other(e.to_string()))
}

#[cfg(test)]
#[path = "drawing_export_tests.rs"]
mod tests;
