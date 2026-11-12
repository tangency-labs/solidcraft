//! `drawing.template.import` (#3875): a drawing template from another program's sheet format, saved
//! as SVG (W3C SVG 1.1; FreeCAD's TechDraw templates are SVG, and most programs export one). The
//! file's line work and text become the template's sheet format ([`SheetFormat`]), drawn under the
//! views as it is; its page becomes the sheet, matched to a standard size within 2 mm. The texts the
//! file marks as fields (`data-field`, or FreeCAD's documented `freecad:editable`) are listed and
//! kept as fixed text for now. The template is an ordinary one: drawing.create and
//! drawing.template.apply take the file it writes.

use std::f64::consts::{FRAC_1_SQRT_2, TAU};
use std::path::Path;
use std::sync::Arc;

use serde_json::Value;
use solidcore_record::{Seg2, Vec2};
use solidcraft_doc::drawing::sheet_format::{self, FormatStroke, FormatText, SheetFormat};
use solidcraft_doc::drawing::{self, template};
use solidcraft_geom::drawing_sheet::{Orientation, PaperSize, Sheet, SheetSize};
use solidcraft_geom::prelude::*;
use solidcraft_io::Geom2;

use super::CommandSpec;
use crate::params::bad;
use crate::schema::drawing_template_import::{ImportParams, ImportResult, ImportedField, ImportedSheet, SkippedCount};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const IMPORT: &str = "drawing.template.import";
const IMPORT_CONTRACT: TypedContract<ImportParams> = TypedContract::new();
/// The most bytes of a path the command takes.
const MAX_PATH_BYTES: usize = 4096;
/// How close a page must be to a standard size to take it, in millimetres.
const MATCH_MM: f64 = 2.0;

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(IMPORT, "Import Drawing Template", crate::checked!("drawing.template.import", import))
    .any_part()
    .icon("open")
    .noundo()
    .doc_only()
    .contract(&IMPORT_CONTRACT)
    .output(output_schema_for::<ImportResult>)
    .params(
        "file: an .svg file, another program's sheet format; path?: the .json template to write (needed unless dry_run); dry_run?: true to answer without writing → {path?, sheet: {size_mm, standard?, orientation}, strokes, texts, fields: [{name, text}], skipped?: [{kind, count}], approximated?: [{kind, count}], dry_run}. The file's page becomes the template's one sheet (a standard size when within 2 mm, else its own), with no SolidCraft border or title block: its line work and text are the sheet's format, drawn under the views as they are. Texts marked as fields (data-field, or freecad:editable) are listed and kept as fixed text. drawing.create and drawing.template.apply take the file written. Changes nothing in the design.",
    )];

fn import(_s: &mut Session, p: &Value) -> Result<Value> {
    let request: ImportParams = crate::schema::read(IMPORT, p)?;
    let svg = |path: &str| path.len() <= MAX_PATH_BYTES && Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("svg"));
    if !svg(&request.file) {
        return Err(bad(IMPORT, "`file` is an .svg file: save the other program's sheet format as SVG (a DXF can be turned into one)"));
    }
    let json = |path: &str| path.len() <= MAX_PATH_BYTES && Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("json"));
    let path = match (&request.path, request.dry_run) {
        (Some(path), _) if !json(path) => return Err(bad(IMPORT, "`path` must be a .json file")),
        (None, false) => return Err(bad(IMPORT, "`path` names the .json template to write (or give dry_run)")),
        (path, _) => path.clone(),
    };
    let bytes =
        solidcraft_io::vfs::read(&request.file, solidcraft_io::MAX_DRAWING_BYTES).map_err(|e| bad(IMPORT, format!("{}: {e}", request.file)))?;
    let text = solidcraft_io::decode_svg(&bytes).map_err(|e| bad(IMPORT, format!("{}: {e}", request.file)))?;
    let (low, high) = solidcraft_io::svg_sheet(&text).ok_or_else(|| {
        bad(IMPORT, format!("{}: the SVG gives its page no width and height or viewBox, so there is no sheet size to take", request.file))
    })?;
    let page = high - low;
    let (geometry, fields) = solidcraft_io::read_svg_with_fields(&text).map_err(|e| bad(IMPORT, format!("{}: {e}", request.file)))?;
    let imported = convert(&geometry, &fields, low);
    let (paper, standard) = paper(page);
    let size_mm = paper.size_mm().map_err(|e| bad(IMPORT, e.to_string()))?;
    let format = SheetFormat { strokes: imported.strokes, texts: imported.texts };
    format.check().map_err(|e| bad(IMPORT, format!("{}: {e}", request.file)))?;
    let made = made(&request.file, paper, format)?;
    let (strokes, texts) = made.sheets.first().map_or((0, 0), |sheet| (sheet.format.strokes.len(), sheet.format.texts.len()));
    if let (Some(path), false) = (&path, request.dry_run) {
        let written = made.to_json().map_err(|e| bad(IMPORT, e.to_string()))?;
        solidcraft_io::vfs::check(path).map_err(|e| EngineError::Other(e.to_string()))?;
        solidcraft_io::write_atomic(Path::new(path), written.as_bytes(), false).map_err(|e| EngineError::Other(e.to_string()))?;
    }
    let reply = ImportResult {
        path: path.filter(|_| !request.dry_run),
        sheet: ImportedSheet {
            size_mm: [size_mm.x, size_mm.y],
            standard,
            orientation: if paper.orientation == Orientation::Portrait { "portrait" } else { "landscape" }.into(),
        },
        strokes,
        texts,
        fields: imported.fields,
        skipped: counts(imported.skipped),
        approximated: counts(imported.approximated),
        dry_run: request.dry_run,
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// The template: the built-in part template's settings, its one sheet `paper` with no border or
/// title block of its own, drawing `format`, named after `file`.
fn made(file: &str, paper: Sheet, format: SheetFormat) -> Result<drawing::Drawing> {
    let mut made = template::built_in("iso-a3-part")
        .ok_or_else(|| bad(IMPORT, "no built-in template to start from"))?
        .map_err(|e| bad(IMPORT, e.to_string()))?;
    made.name = Path::new(file).file_stem().map(|stem| stem.to_string_lossy().chars().take(100).collect()).unwrap_or_default();
    for sheet in &mut made.sheets {
        let sheet = Arc::make_mut(sheet);
        sheet.sheet = paper;
        sheet.border = None;
        sheet.set_title_block(None);
        sheet.format = format.clone();
    }
    template::check(&made).map_err(|e| bad(IMPORT, e.to_string()))?;
    Ok(made)
}

/// The sheet a page `page` mm wide and high takes: the standard size within [`MATCH_MM`] of it,
/// lying as it does, else a size of its own; and that standard size's name.
fn paper(page: Vec2) -> (Sheet, Option<String>) {
    let orientation = if page.y > page.x { Orientation::Portrait } else { Orientation::Landscape };
    let (short, long) = (page.x.min(page.y), page.x.max(page.y));
    let matched = PaperSize::ALL.into_iter().find(|size| {
        let (a, b) = size.sides_mm();
        (a - short).abs() <= MATCH_MM && (b - long).abs() <= MATCH_MM
    });
    match matched {
        Some(size) => {
            let name = serde_json::to_value(size).ok().and_then(|v| v.as_str().map(str::to_string));
            (Sheet::standard(size, orientation), name)
        }
        None => (Sheet { size: SheetSize::Custom { sides_mm: [page.x, page.y] }, orientation }, None),
    }
}

/// What a file's geometry becomes.
#[derive(Default)]
struct Imported {
    strokes: Vec<FormatStroke>,
    texts: Vec<FormatText>,
    fields: Vec<ImportedField>,
    skipped: Vec<&'static str>,
    approximated: Vec<&'static str>,
}

/// `geometry` (millimetres, y up) moved onto the sheet: the page's lower-left corner, `low` in the
/// geometry's coordinates, to the origin. `fields` names the texts marked as fields by their place in it.
fn convert(geometry: &[Geom2], fields: &[(usize, String)], low: Vec2) -> Imported {
    let on = |v: Vec2| v - low;
    let mut out = Imported::default();
    let stroke = |path: Vec<Seg2>, closed: bool| FormatStroke { path, closed, width_mm: None };
    for (i, g) in geometry.iter().enumerate() {
        match g {
            Geom2::Point(_) => out.skipped.push("point"),
            Geom2::Line(a, b) => out.strokes.push(stroke(vec![Seg2::Line { a: on(*a), b: on(*b) }], false)),
            Geom2::Circle(c, r) => out.strokes.push(stroke(vec![Seg2::Arc { center: on(*c), radius: *r, start: 0.0, sweep: TAU }], true)),
            Geom2::Arc { c, a, b } => {
                let (start, end) = ((*a - *c).angle(), (*b - *c).angle());
                let sweep = (end - start).rem_euclid(TAU);
                let sweep = if sweep < 1e-12 { TAU } else { sweep };
                out.strokes.push(stroke(vec![Seg2::Arc { center: on(*c), radius: (*a - *c).len(), start, sweep }], false));
            }
            Geom2::Ellipse { c, major, minor } => {
                // Four quarter conics, each exact with its middle weight 1/sqrt(2).
                let m = *major;
                let n = m.normalized().map_or(Vec2::ZERO, |u| u.perp() * *minor);
                let corners = [m, n, -m, -n];
                let path = (0..4)
                    .map(|q| {
                        let (p, r) = (corners[q], corners[(q + 1) % 4]);
                        Seg2::Conic { a: on(*c + p), apex: on(*c + p + r), b: on(*c + r), w: FRAC_1_SQRT_2 }
                    })
                    .collect();
                out.strokes.push(stroke(path, true));
            }
            Geom2::Conic { a, apex, b, rho } if *rho > 0.0 && *rho < 1.0 => {
                out.strokes.push(stroke(vec![Seg2::Conic { a: on(*a), apex: on(*apex), b: on(*b), w: rho / (1.0 - rho) }], false));
            }
            Geom2::Conic { .. } => out.skipped.push("curve"),
            Geom2::Spline { pts, .. } => {
                let path: Vec<Seg2> = pts.windows(2).map(|w| Seg2::Line { a: on(w[0]), b: on(w[1]) }).collect();
                if path.is_empty() {
                    out.skipped.push("curve");
                } else {
                    out.approximated.push("spline");
                    out.strokes.push(stroke(path, false));
                }
            }
            Geom2::Text { text, at, height, angle } => {
                let cap_mm = *height;
                if !sheet_format::CAP_MM.contains(&cap_mm) {
                    out.skipped.push("text too small or large to print");
                    continue;
                }
                if let Some((_, name)) = fields.iter().find(|(at, _)| *at == i) {
                    out.fields.push(ImportedField { name: name.clone(), text: text.clone() });
                }
                let text: String = text.chars().filter(|c| !c.is_control()).take(sheet_format::MAX_TEXT_CHARS).collect();
                if text.trim().is_empty() {
                    out.skipped.push("empty text");
                    continue;
                }
                out.texts.push(FormatText { at: [on(*at).x, on(*at).y], text, cap_mm, angle_deg: angle.to_degrees() });
            }
        }
    }
    out
}

/// Each kind once, with how many times it came, in the order they first came.
fn counts(kinds: Vec<&'static str>) -> Vec<SkippedCount> {
    let mut out: Vec<SkippedCount> = Vec::new();
    for kind in kinds {
        match out.iter_mut().find(|c| c.kind == kind) {
            Some(c) => c.count += 1,
            None => out.push(SkippedCount { kind: kind.into(), count: 1 }),
        }
    }
    out
}

#[cfg(test)]
#[path = "drawing_template_import_tests.rs"]
mod tests;
