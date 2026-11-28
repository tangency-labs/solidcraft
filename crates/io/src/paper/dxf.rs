//! A paper scene as DXF (#4890): one sheet in millimetres ($INSUNITS 4), at its place on the
//! paper, through the shared [`crate::DxfFile`] writer, with a layer for each kind of line.
//!
//! - **Layers:** a stroke is on `VISIBLE` (a wide line: visible edges, borders), `THIN` (a narrow
//!   line: dimensions, leaders, centre lines, a reference part's edges), `HIDDEN` (dashed in two
//!   lengths: hidden edges, the `HIDDEN` linetype) or `DASHED` (any other pattern, such as a chain
//!   line, the `DASHED` linetype). Wide or narrow is read against the line group the caller gives
//!   for each object: a solid stroke at least three quarters of its wide line is wide (the narrow
//!   line is half of it). Text is on `TEXT`, fills (arrowheads, solid hatching) on `FILL`. Only the
//!   layers a sheet uses are declared, in that order.
//! - **Weights:** each layer carries its kind's ISO 128 width, and each entity its own stroke width
//!   as the nearest standard DXF lineweight, so a reader that shows weights shows the sheet's.
//! - **Curves** go through the one path converter ([`crate::dxf_out::path_shapes`], polyline
//!   style): lines and circular arcs become polylines with bulges, other curves keep their exact
//!   form.
//! - **Text** is its glyph outlines as closed polylines, as SVG and PDF draw it, so it stands where
//!   the sheet shows it without the font.
//! - **Fills** are their boundary loops as closed polylines: DXF has no fill this writer uses.
//! - **Pictures** (shaded views, images) can't be lines: they are left out and counted in the
//!   report.

use solidcore_record::Seg2;
use solidcraft_geom::paper::{Item, ObjectKind, Prepared};

use crate::dxf_out::{
    DxfEntityWeight as EntityWeight, DxfFile, DxfLayerId as LayerId, DxfLinetype, DxfPath, DxfPathStyle, DxfShape,
    DxfStandardWeight as StandardWeight, path_shapes, write_dxf_file,
};
use crate::{IoError, Result};

/// What writing a sheet's DXF did that the scene didn't say.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DxfReport {
    /// Pictures left out: a DXF holds only lines.
    pub pictures_left_out: usize,
}

/// A sheet DXF's layers, in the order they are declared: (name, colour, linetype).
const LAYERS: [(&str, i16, DxfLinetype); 6] = [
    ("VISIBLE", 7, DxfLinetype::Continuous),
    ("THIN", 4, DxfLinetype::Continuous),
    ("HIDDEN", 8, DxfLinetype::Hidden),
    ("DASHED", 1, DxfLinetype::Dashed),
    ("TEXT", 2, DxfLinetype::Continuous),
    ("FILL", 7, DxfLinetype::Continuous),
];

/// Which of [`LAYERS`] a stroke is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layer {
    Visible,
    Thin,
    Hidden,
    Dashed,
    Text,
    Fill,
}

/// The least width of a wide line on the sheet's own furniture: its border, title block and tables
/// are drawn in fixed ISO 5457 widths (0.7 mm outlines, 0.35 mm grid and cell lines), whatever
/// line group the views use, so they are read against this, not against the group's wide line.
const FURNITURE_WIDE_MM: f64 = 0.5;

/// Whether `kind` is the sheet's own furniture, drawn in fixed widths.
fn is_furniture(kind: ObjectKind) -> bool {
    matches!(kind, ObjectKind::SheetBorder | ObjectKind::DrawingBorder | ObjectKind::TitleBlock | ObjectKind::PartsList | ObjectKind::RevisionTable)
}

/// The layer of a stroke `width_mm` wide, dashed in `pattern` lengths, in a line group whose wide
/// line is `wide_mm`. `furniture` strokes are wide from [`FURNITURE_WIDE_MM`] up.
fn stroke_layer(width_mm: f64, pattern: Option<usize>, wide_mm: f64, furniture: bool) -> Layer {
    match pattern {
        Some(2) => Layer::Hidden,
        Some(_) => Layer::Dashed,
        None if furniture && width_mm >= FURNITURE_WIDE_MM => Layer::Visible,
        None if furniture => Layer::Thin,
        None if width_mm >= 0.75 * wide_mm => Layer::Visible,
        None => Layer::Thin,
    }
}

/// A stroke width as its entity's weight: the nearest standard lineweight, or the layer's for a
/// width past the heaviest one.
fn weight(width_mm: f64) -> Option<EntityWeight> {
    StandardWeight::from_mm(width_mm).ok().map(EntityWeight::Explicit)
}

/// `scene` as a DXF document, its strokes read against the wide line `wide_mm(object)` gives for
/// each object (above 0), and what writing it did.
pub fn paper_dxf(scene: &Prepared, wide_mm: &dyn Fn(u64) -> f64) -> Result<(String, DxfReport)> {
    let mut file: DxfFile<DxfShape> = DxfFile::new()?;
    let mut declared: [Option<LayerId>; 6] = [None; 6];
    let mut report = DxfReport::default();
    let mut layer = |file: &mut DxfFile<DxfShape>, which: Layer, wide: f64| -> Result<LayerId> {
        let at = which as usize;
        if let Some(id) = declared[at] {
            return Ok(id);
        }
        let (name, colour, linetype) = LAYERS[at];
        let width = match which {
            Layer::Visible => wide,
            _ => wide / 2.0,
        };
        let id = file.declare_layer(name, colour, linetype, StandardWeight::from_mm(width).ok())?;
        declared[at] = Some(id);
        Ok(id)
    };
    let push = |file: &mut DxfFile<DxfShape>, layer: LayerId, shapes: Vec<DxfShape>, weight: Option<EntityWeight>| -> Result<()> {
        for shape in shapes {
            file.push(layer, shape, weight)?;
        }
        Ok(())
    };
    for group in scene.groups() {
        let wide = wide_mm(group.object);
        if !(wide.is_finite() && wide > 0.0) {
            return Err(IoError::Invalid(format!("a sheet DXF's wide line is above 0 mm, not {wide}")));
        }
        for item in &group.items {
            match item {
                Item::Stroke { path, closed, width_mm, pattern } => {
                    let on = layer(
                        &mut file,
                        stroke_layer(*width_mm, pattern.as_ref().map(|p| p.lengths_mm.len()), wide, is_furniture(group.kind)),
                        wide,
                    )?;
                    let shapes = if *closed {
                        let lp = solidcore_record::Loop2 { segs: path.clone() };
                        path_shapes(DxfPath::Closed(&lp), DxfPathStyle::Polyline)?
                    } else {
                        path_shapes(DxfPath::Open(path), DxfPathStyle::Polyline)?
                    };
                    push(&mut file, on, shapes, weight(*width_mm))?;
                }
                Item::Fill { region } => {
                    let on = layer(&mut file, Layer::Fill, wide)?;
                    for lp in region.loops() {
                        push(&mut file, on, path_shapes(DxfPath::Closed(lp), DxfPathStyle::Polyline)?, None)?;
                    }
                }
                Item::Text(run) => {
                    let font = scene.font(run).ok_or_else(|| IoError::Invalid("a sheet DXF: a text run's font is missing".into()))?;
                    let on = layer(&mut file, Layer::Text, wide)?;
                    for lp in run.outlines(font) {
                        if lp.segs.iter().all(|s| matches!(s, Seg2::Line { a, b } if a == b)) {
                            continue;
                        }
                        push(&mut file, on, path_shapes(DxfPath::Closed(&lp), DxfPathStyle::Polyline)?, None)?;
                    }
                }
                Item::Picture(_) => report.pictures_left_out += 1,
            }
        }
    }
    Ok((write_dxf_file(&file)?, report))
}

#[cfg(test)]
#[path = "dxf_tests.rs"]
mod tests;
