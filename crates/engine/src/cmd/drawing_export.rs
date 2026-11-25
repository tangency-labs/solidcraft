//! `drawing.export` (#4890 A7): one sheet of the open drawing written out from its scene
//! ([`crate::drawing_scene::build`]), the scene the sheet page draws, so a file shows what the page
//! does: SVG, PNG at a chosen dpi and colour, or a vector PDF at the sheet's paper size, which can
//! hold several sheets, one page each, or DXF in millimetres with a layer for each kind of line. A
//! sheet with a view or sketch that failed, an image that won't decode or a picture that can't be drawn isn't
//! written; the refusal names them. Without a path the reply holds an SVG's or DXF's text; a PNG or
//! PDF is a file and needs a path (the app's own print takes the PDF's bytes in-process,
//! [`print_pdf`]). Both can be on a blue paper (#10679).

use std::path::Path;

use serde_json::Value;
use solidcraft_doc::drawing::DEFAULT_DPI;
use solidcraft_doc::drawing::Drawing;
use solidcraft_doc::ids;
use solidcraft_geom::paper::line::LineKind;
use solidcraft_geom::paper::{GridReport, Pictures, Prepared};
use solidcraft_io::MAX_DRAWING_BYTES;
use solidcraft_io::paper::{DPI, PaperColour, PdfOptions, PdfPictures, PngColour, PngOptions};

use super::CommandSpec;
use crate::drawing_pictures::SheetPictures;
use crate::drawing_scene::{FillStatus, SheetScene, SketchStatus, Status, sheet_id};
use crate::params::bad;
use crate::schema::drawing_export::{ExportColour, ExportFormat, ExportParams, ExportReport, ExportResult, PrintParams};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const CMD: &str = "drawing.export";
const CONTRACT: TypedContract<ExportParams> = TypedContract::new();
const PRINT: &str = "drawing.print";
const PRINT_CONTRACT: TypedContract<PrintParams> = TypedContract::new();
/// The longest path taken, in bytes, as the view DXF's.
const MAX_PATH_BYTES: usize = 4096;

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(CMD, "Export Sheet", crate::checked!("drawing.export", export))
    .at("DRAWING REVIEW", "OUTPUT")
    .icon("export")
    .in_drawing()
    .noundo()
    .doc_only()
    .contract(&CONTRACT)
    .output(output_schema_for::<ExportResult>)
    .params("sheet: id or name; format?: svg (default) | png | pdf | dxf (millimetres, layers VISIBLE, THIN, HIDDEN, DASHED, TEXT and FILL, each entity at its line's weight; pictures are left out and counted in report.pictures_left_out); pdf only: sheets?: [id or name, ...], one page each in that order (a pdf given neither sheet nor sheets holds every sheet, in the drawing's order), each page the sheet's paper size, its lines and text vector, shaded views and images embedded; path?: a file with the format's extension, required for png and pdf (else the reply holds an svg's or dxf's text, at most 64 MiB); png only: dpi?: 25 to 2400 (default the sheet's own), colour?: grey (default) | black_white | rgb | transparent; paper?: white (default) | blueprint | prussian_blue, whose png is rgb → {sheet, pages? (a pdf's sheets), format, bytes, path | svg | dxf, report: {widths_snapped, pieces_merged, pixels?: [width, height], pictures_left_out?}, views: [{view, status: drawn|placeholder|failed, reason?, stats?}]}. The sheet as drawing.scene draws it: its drawing border, each image on it, then each view's lines, then centre annotations, parts lists and revision tables, then its title block's lines. Refused, naming them, while any view or sketch on the sheet failed or any fill failed or lost its boundary or any image won't decode; a placeholder view draws nothing. Omitted centre annotations are listed in annotation_failures with their reasons. A view of another configuration evaluates a copy of the design; the design is unchanged."),
    CommandSpec::new(PRINT, "Print Drawing", crate::checked!("drawing.print", print))
        .at("DRAWING REVIEW", "OUTPUT")
        .icon("export")
        .key("Ctrl+P")
        .in_drawing()
        .noundo()
        .doc_only()
        .contract(&PRINT_CONTRACT)
        .output(output_schema_for::<ExportResult>)
        .params("path: a .pdf file → as drawing.export's for a pdf. The whole drawing as one vector PDF for printing, one page per sheet in the drawing's order, each page its sheet's paper size and asking a viewer to print it at actual size (PrintScaling None): drawing.export with format pdf and no sheets named. The window opens it in the system's PDF viewer (a new tab in a browser) to print from there."),
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

/// A sheet drawn for a file. Refused, naming them, while any view, sketch or fill on it failed.
fn drawn_sheet(s: &mut Session, d: &Drawing, sheet: u64) -> Result<SheetScene> {
    let built = crate::drawing_scene::as_shown(s, d, sheet)?;
    let failed: Vec<String> = built
        .views
        .iter()
        .filter_map(|v| match &v.status {
            Status::Failed(why) => Some(format!("{} ({why})", ids::encode(v.view))),
            _ => None,
        })
        .collect();
    let failed_sketches: Vec<String> = built
        .sketches
        .iter()
        .filter_map(|sketch| match &sketch.status {
            SketchStatus::Failed(why) => Some(format!("{} ({why})", ids::encode(sketch.sketch))),
            SketchStatus::Drawn => None,
        })
        .collect();
    let failed_fills: Vec<String> = built
        .fills
        .iter()
        .filter_map(|fill| match &fill.status {
            FillStatus::Drawn => None,
            FillStatus::LostBoundary(reason) => Some(format!("{} (lost boundary: {reason})", ids::encode(fill.fill))),
            FillStatus::Failed(reason) => Some(format!("{} ({reason})", ids::encode(fill.fill))),
        })
        .collect();
    let mut failures = Vec::new();
    if !failed.is_empty() {
        failures.push(format!("views on the sheet failed: {}", failed.join(", ")));
    }
    if !failed_sketches.is_empty() {
        failures.push(format!("sketches on the sheet failed: {}", failed_sketches.join(", ")));
    }
    if !failed_fills.is_empty() {
        failures.push(format!("fills on the sheet failed or lost their boundary: {}", failed_fills.join(", ")));
    }
    if !failures.is_empty() {
        return Err(bad(CMD, format!("not written: {}", failures.join("; "))));
    }
    Ok(built)
}

/// The sheets a call names, in its order: `sheet`, or a PDF's `sheets`, or every sheet for a PDF
/// that names none.
fn chosen_sheets(d: &Drawing, request: &ExportParams) -> Result<Vec<u64>> {
    let pdf = request.format == ExportFormat::Pdf;
    match (&request.sheet, &request.sheets) {
        (Some(_), Some(_)) => Err(bad(CMD, "give `sheet` or `sheets`, not both")),
        (Some(sheet), None) => Ok(vec![sheet_id(d, sheet, CMD)?]),
        (None, Some(_)) if !pdf => Err(bad(CMD, "`sheets` is for pdf only: give one `sheet`")),
        (None, Some(sheets)) => {
            if sheets.is_empty() {
                return Err(bad(CMD, "`sheets` names at least one sheet"));
            }
            let mut ids = Vec::with_capacity(sheets.len().min(d.sheets.len()));
            for named in sheets {
                let id = sheet_id(d, named, CMD)?;
                if ids.contains(&id) {
                    return Err(bad(CMD, format!("`sheets` names sheet `{named}` twice")));
                }
                ids.push(id);
            }
            Ok(ids)
        }
        (None, None) if pdf => {
            if d.sheets.is_empty() {
                return Err(bad(CMD, "the drawing has no sheets"));
            }
            Ok(d.sheets.iter().map(|sheet| sheet.id).collect())
        }
        (None, None) => Err(bad(CMD, "`sheet` is needed: give its id or name")),
    }
}

/// `drawing.print`: the whole drawing's PDF, as `drawing.export` writes it, asking a viewer to
/// print it at actual size.
fn print(s: &mut Session, p: &Value) -> Result<Value> {
    let request: PrintParams = crate::schema::read(PRINT, p)?;
    let mut params = serde_json::json!({"format": "pdf"});
    if let Some(path) = request.path {
        params["path"] = Value::String(path);
    }
    export_into(s, &params, None, true)
}

/// The open drawing's PDF for printing, every sheet a page, as `drawing.print` writes it: its bytes,
/// for the app to hand to a viewer, never as text.
pub fn print_pdf(s: &mut Session) -> Result<Vec<u8>> {
    let mut document = Vec::new();
    export_into(s, &serde_json::json!({"format": "pdf"}), Some(&mut document), true)?;
    Ok(document)
}

fn export(s: &mut Session, p: &Value) -> Result<Value> {
    export_into(s, p, None, false)
}

/// `drawing.export`; with `bytes` and no path, the document goes there rather than to a file or the
/// reply. A PDF asks a viewer to print it at actual size when `actual_size` (Print), and leaves the
/// viewer its own choice otherwise (Export), as it always has.
fn export_into(s: &mut Session, p: &Value, bytes_out: Option<&mut Vec<u8>>, actual_size: bool) -> Result<Value> {
    let request: ExportParams = crate::schema::read(CMD, p)?;
    let d = s.drawing(CMD)?.clone();
    let sheets = chosen_sheets(&d, &request)?;
    let sheet = sheets[0];
    let paper: Option<PaperColour> = request.paper.into();
    let png = match request.format {
        ExportFormat::Svg => None,
        ExportFormat::Pdf => {
            if paper.is_some() {
                return Err(bad(CMD, "a pdf is on white paper: `paper` is for svg and png"));
            }
            None
        }
        ExportFormat::Png => {
            // The sheet's own resolution unless the call gives one.
            let own = d.sheets.iter().find(|s| s.id == sheet).map(|s| s.dpi);
            let dpi = request.dpi.or(own).unwrap_or(PngOptions::default().dpi);
            if !DPI.contains(&dpi) {
                return Err(bad(CMD, "`dpi` is 25 to 2400"));
            }
            let colour = match (paper, request.colour) {
                (None, colour) => colour.unwrap_or_default().into(),
                (Some(paper), None | Some(ExportColour::Rgb)) => PngColour::On(paper),
                (Some(_), Some(_)) => return Err(bad(CMD, "a blue `paper` is rgb: `grey`, `black_white` and `transparent` are for white paper")),
            };
            Some(PngOptions { dpi, colour })
        }
        ExportFormat::Dxf => {
            if paper.is_some() {
                return Err(bad(CMD, "a dxf has no paper: `paper` is for svg and png"));
            }
            None
        }
    };
    if png.is_none() && (request.dpi.is_some() || request.colour.is_some()) {
        return Err(bad(CMD, "`dpi` and `colour` are for png only"));
    }
    let extension = request.format.extension();
    // A PNG or PDF is a file: it is written to one, never put in the reply as text.
    if request.path.is_none() && bytes_out.is_none() && matches!(request.format, ExportFormat::Png | ExportFormat::Pdf) {
        return Err(bad(CMD, format!("a {} is written to a file: give `path`", if request.format == ExportFormat::Png { "PNG" } else { "PDF" })));
    }
    if let Some(path) = &request.path {
        let named = Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case(extension));
        if path.len() > MAX_PATH_BYTES || !named {
            return Err(bad(CMD, format!("path must be a .{extension} file")));
        }
        solidcraft_io::vfs::check(path).map_err(|e| EngineError::Other(e.to_string()))?;
    }
    let mut drawn = Vec::with_capacity(sheets.len());
    for &sheet in &sheets {
        drawn.push(drawn_sheet(s, &d, sheet)?);
    }
    let mut pictures = Vec::with_capacity(sheets.len());
    for &sheet in &sheets {
        pictures.push(SheetPictures::new(s, &d, sheet).map_err(|e| bad(CMD, e))?);
    }
    let mut scenes: Vec<Prepared> = Vec::with_capacity(drawn.len());
    for built in &drawn {
        scenes.push(built.scene.prepare().map_err(|e| bad(CMD, e.to_string()))?);
    }
    let prepared = &scenes[0];
    let mut pictures_left_out = None;
    let (document, grid, pixels) = match (request.format, png) {
        (ExportFormat::Dxf, _) => {
            // Each stroke is read against its own view's line group, or its sheet's.
            let sheet_group = d.sheets.iter().find(|s| s.id == sheet).and_then(|s| s.line_group).unwrap_or(d.line_group);
            let wide = |object: u64| {
                let group = d.resolve_view(object).map_or(sheet_group, |view| view.line_group.value);
                LineKind::Visible.width_mm(group)
            };
            let (dxf, report) = solidcraft_io::paper::paper_dxf(prepared, &wide).map_err(|e| bad(CMD, e.to_string()))?;
            pictures_left_out = Some(report.pictures_left_out);
            (dxf.into_bytes(), GridReport::default(), None)
        }
        (ExportFormat::Pdf, _) => {
            let sources: Vec<&dyn Pictures> = pictures.iter().map(|p| p as &dyn Pictures).collect();
            let options = PdfOptions { title: Some(d.name.clone()).filter(|name| !name.is_empty()), output_time: None, layers: true, actual_size };
            let (pdf, reports) =
                solidcraft_io::paper::paper_pdf_pages_with(&scenes, &options, Some(PdfPictures { pages: &sources, dpi: DEFAULT_DPI }))
                    .map_err(|e| bad(CMD, e.to_string()))?;
            let mut grid = GridReport::default();
            for report in reports {
                grid.widths_snapped += report.widths_snapped;
                grid.pieces_merged += report.pieces_merged;
            }
            (pdf, grid, None)
        }
        (_, None) => {
            let (svg, grid) = solidcraft_io::paper::paper_svg_on(prepared, paper, &pictures[0], DEFAULT_DPI).map_err(|e| bad(CMD, e.to_string()))?;
            (svg, grid, None)
        }
        (_, Some(options)) => {
            // Bounded only for the reply: a file may be as large as the sheet needs.
            let limit = if request.path.is_some() { usize::MAX } else { MAX_DRAWING_BYTES };
            let mut held = Held { bytes: Vec::new(), limit };
            let written = solidcraft_io::paper::paper_png(prepared, options, &pictures[0], &mut held).map_err(|e| bad(CMD, e.to_string()))?;
            (held.bytes, written.grid, Some(written.pixels))
        }
    };
    drop(pictures);
    drop(scenes);
    let bytes = document.len();
    if request.path.is_none() && bytes_out.is_none() && bytes > MAX_DRAWING_BYTES {
        return Err(bad(CMD, format!("over {} MiB for the reply: give a path", MAX_DRAWING_BYTES >> 20)));
    }
    let (path, svg) = match (request.path, bytes_out) {
        (Some(path), _) => {
            // Checked again just before writing, as the view DXF does.
            solidcraft_io::vfs::check(&path).map_err(|e| EngineError::Other(e.to_string()))?;
            solidcraft_io::write_atomic(Path::new(&path), &document, false).map_err(|e| EngineError::Other(e.to_string()))?;
            (Some(path), None)
        }
        (None, Some(out)) => {
            *out = document;
            (None, None)
        }
        (None, None) => (None, Some(String::from_utf8(document).map_err(|e| EngineError::Other(e.to_string()))?)),
    };
    let (svg, dxf) = if request.format == ExportFormat::Dxf { (None, svg) } else { (svg, None) };
    let pages = if request.format == ExportFormat::Pdf { sheets.iter().map(|id| ids::encode(*id)).collect() } else { Vec::new() };
    let mut reply = ExportResult {
        sheet: ids::encode(sheet),
        pages,
        format: request.format,
        bytes,
        path,
        svg,
        dxf,
        report: ExportReport { widths_snapped: grid.widths_snapped, pieces_merged: grid.pieces_merged, pixels, pictures_left_out },
        views: Vec::new(),
        dimensions: Vec::new(),
        notes: Vec::new(),
        annotation_failures: Vec::new(),
        balloons: Vec::new(),
        tables: Vec::new(),
        out_of_date: Vec::new(),
        sketches: Vec::new(),
        fills: Vec::new(),
        param_warnings: Vec::new(),
    };
    for built in drawn {
        reply.views.extend(built.views.into_iter().map(Into::into));
        reply.dimensions.extend(super::drawing_scene::dimension_results(built.dimensions));
        reply.notes.extend(super::drawing_scene::note_results(built.notes));
        reply.annotation_failures.extend(built.annotation_failures.into_iter().map(Into::into));
        reply.balloons.extend(super::drawing_scene::balloon_results(built.balloons));
        reply.tables.extend(super::drawing_scene::table_results(built.tables));
        reply.out_of_date.extend(built.out_of_date.iter().map(|(id, _)| ids::encode(*id)));
        reply.sketches.extend(built.sketches.into_iter().map(Into::into));
        reply.fills.extend(built.fills.into_iter().map(Into::into));
    }
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

#[cfg(test)]
#[path = "drawing_export_tests.rs"]
mod tests;
