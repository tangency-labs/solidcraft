//! Markdown diagnostics and saved-coordinate sketch pictures. Export never changes the design.

use serde_json::{Value, json};
use solidcraft_geom::Vec2;
use solidcraft_io::{FreeCadSketchPreview, FreeCadWarning, FreeCadWarningSeverity, vfs};
use solidcraft_render::{SketchIssueCurve, sketch_issue_png};

use crate::{Result, Session, params::bad};

const CMD: &str = "file.freecad_import_report";
const MAX_BYTES: usize = 16 << 20;
const MAX_IMAGES: usize = 32;

#[derive(serde::Deserialize)]
struct Report {
    project: String,
    source: String,
    summary: String,
    warnings: Vec<FreeCadWarning>,
    #[serde(default)]
    previews: Vec<FreeCadSketchPreview>,
}

fn md(value: &str) -> String {
    let mut text = String::new();
    for ch in value.chars() {
        if "\\`*_{}[]<>#!|".contains(ch) {
            text.push('\\');
        }
        text.push(ch);
    }
    text
}

fn url(value: &str) -> String {
    value.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._/".contains(&b) { char::from(b).to_string() } else { format!("%{b:02X}") }).collect()
}

fn picture(preview: &FreeCadSketchPreview, issue: &FreeCadWarning, number: usize) -> (Option<Vec<u8>>, String) {
    let Some(context) = &issue.context else { return (None, "No source entity references are available for this issue.".into()) };
    let mut points = Vec::new();
    let mut axes = [false; 2];
    let mut unavailable = Vec::new();
    let curves: Vec<_> = preview
        .curves
        .iter()
        .map(|curve| {
            let highlighted = context.references.iter().any(|(index, _)| usize::try_from(*index).ok() == Some(curve.geometry));
            SketchIssueCurve { points: curve.points.clone(), construction: curve.construction, highlighted }
        })
        .collect();
    for &(index, position) in &context.references {
        if index == -1 && position == 1 {
            points.push(Vec2::new(0.0, 0.0));
        } else if index == -1 || index == -2 {
            axes[usize::from(index == -2)] = true;
        } else if let Some(curve) = preview.curves.iter().find(|curve| usize::try_from(index).ok() == Some(curve.geometry)) {
            let point = match position {
                1 => curve.start,
                2 => curve.end,
                3 => curve.center,
                _ => None,
            };
            if let Some(point) = point {
                points.push(point);
            } else if position != 0 {
                unavailable.push(format!("point {position} on geometry {}", i64::from(index) + 1));
            }
        } else {
            unavailable.push(if index < 0 { format!("external reference {index}") } else { format!("geometry {}", i64::from(index) + 1) });
        }
    }
    let kind = if issue.severity == FreeCadWarningSeverity::Warning { "warning" } else { "issue" };
    let mut caption = if context.references.is_empty() {
        format!(
            "Sketch overview only: this {kind} does not identify a defective curve or point, so nothing is highlighted. The picture shows local sketch geometry; support objects and 3D attachment are not shown. Read the {kind} explanation above for the attachment or placement limitation."
        )
    } else {
        format!(
            "Saved source sketch coordinates. Red curves and rings identify available referenced entities; numbers match this report's {kind} number. Pale lines show the sketch axes. This is diagnostic context, not a repaired sketch."
        )
    };
    if !unavailable.is_empty() {
        caption.push_str(&format!(" Not shown: {}. No position has been inferred for these references.", unavailable.join(", ")));
        if points.is_empty() && !axes.iter().any(|axis| *axis) && !curves.iter().any(|curve| curve.highlighted) {
            caption.push_str(" No referenced entity could be located; this image shows sketch context only.");
        }
    }
    if preview.omitted > 0 {
        caption
            .push_str(&format!(" Source entities omitted from this preview: {} (unsupported, invalid, or beyond the size limit).", preview.omitted));
    }
    (sketch_issue_png(&curves, &points, axes, number), caption)
}

pub(super) fn report(_: &mut Session, params: &Value) -> Result<Value> {
    let value = params.get("report").ok_or_else(|| bad(CMD, "`report` is required"))?;
    let bytes = serde_json::to_vec(value).map_err(|error| bad(CMD, error.to_string()))?;
    if bytes.len() > MAX_BYTES {
        return Err(bad(CMD, "the report exceeds 16 MB"));
    }
    let report: Report = serde_json::from_slice(&bytes).map_err(|error| bad(CMD, format!("invalid report: {error}")))?;
    let path = if params.get("path").is_some() { Some(super::path_arg(params, CMD)?) } else { None };
    let images = match params.get("images") {
        None => true,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err(bad(CMD, "`images` must be true or false")),
    };
    // Validate preview shape before rendering or allocating raster buffers.
    if report.warnings.len() > 10_000
        || report.previews.len() > 200
        || report.previews.iter().any(|p| p.curves.len() > 256 || p.curves.iter().any(|c| c.points.len() > 128))
        || report.warnings.iter().any(|w| w.context.as_ref().is_some_and(|c| c.references.len() > 3))
    {
        return Err(bad(CMD, "the report exceeds diagnostic preview limits"));
    }
    let mut text =
        format!("# FreeCAD Import Report\n\nProject: {}\n\nSource: {}\n\n{}\n\n", md(&report.project), md(&report.source), md(&report.summary));
    if report.warnings.is_empty() {
        text.push_str("No import issues or warnings were reported.\n");
    }
    let mut assets = Vec::new();
    let mut image_errors = Vec::new();
    let mut attempted_images = 0;
    let mut issue_number = 0;
    let mut warning_number = 0;
    for issue in &report.warnings {
        let (kind, number) = if issue.severity == FreeCadWarningSeverity::Warning {
            warning_number += 1;
            ("Warning", warning_number)
        } else {
            issue_number += 1;
            ("Issue", issue_number)
        };
        text.push_str(&format!("## {kind} {number}: {}\n\n{}\n\n", md(&issue.summary), md(&issue.detail)));
        let preview = issue.context.as_ref().and_then(|c| report.previews.iter().find(|p| p.source == c.sketch));
        let wants_image =
            issue.severity == FreeCadWarningSeverity::Issue || issue.context.as_ref().is_some_and(|context| !context.references.is_empty());
        if images
            && wants_image
            && let Some(path) = path
            && let Some(preview) = preview
        {
            if attempted_images >= MAX_IMAGES {
                text.push_str("Image omitted: the report's 32-image limit was reached.\n\n");
                continue;
            }
            attempted_images += 1;
            let (png, caption) = picture(preview, issue, number);
            let saved = png.ok_or_else(|| "No renderable source geometry is available.".to_string()).and_then(|png| {
                let destination = std::path::Path::new(path);
                let folder_name = format!("{}-images", destination.file_stem().and_then(|s| s.to_str()).unwrap_or("import-report"));
                let parent = destination.parent().unwrap_or_else(|| std::path::Path::new("."));
                let folder = parent.join(&folder_name);
                vfs::create_dir_all(&folder.to_string_lossy()).map_err(|e| e.to_string())?;
                let file_name = format!("{}-{number:03}.png", kind.to_ascii_lowercase());
                let image_path = folder.join(&file_name);
                vfs::write(&image_path.to_string_lossy(), &png).map_err(|e| e.to_string())?;
                assets.push(image_path.to_string_lossy().to_string());
                Ok(url(&format!("{folder_name}/{file_name}")))
            });
            match saved {
                Ok(link) => text.push_str(&format!("![Sketch context for {kind} {number}]({link})\n\n{}\n\n", md(&caption))),
                Err(error) => {
                    image_errors.push(format!("{kind} {number}: {error}"));
                    text.push_str(&format!("Image unavailable: {}\n\n", md(&error)));
                }
            }
        } else if images && wants_image && issue.context.is_some() && preview.is_none() {
            text.push_str("Image unavailable: saved sketch geometry was not retained for this issue.\n\n");
        }
    }
    if text.len() > MAX_BYTES {
        return Err(bad(CMD, "the formatted report exceeds 16 MB"));
    }
    if let Some(path) = path {
        vfs::write(path, text.as_bytes()).map_err(|error| crate::EngineError::Other(format!("{path}: {error}")))?;
    }
    Ok(json!({"text":text, "path":path, "images":assets, "image_errors":image_errors}))
}
