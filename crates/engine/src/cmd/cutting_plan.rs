//! Cut-list stock allocation; layouts are derived, never saved in the design.
use super::CommandSpec;
use crate::params::bad;
use crate::schema::cutlist::CutList;
use crate::schema::cutting_plan::*;
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};
use serde_json::{Value, json};
use solidcraft_io::workshop::{Kind, Page, Rectangle as SvgRect, Text};
use solidcraft_nest::{Part, Rotation, Settings};
use std::collections::BTreeMap;

const ID: &str = "cutlist.layout";
const CONTRACT: TypedContract<LayoutParams> = TypedContract::new();
pub const METRIC: &str = include_str!("../../../../assets/stock/metric.json");

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("cutlist.layout", "Cutting Plan", crate::checked!("cutlist.layout", layout))
        .at("SOLID", "INSPECT")
        .icon("bom")
        .noundo()
        .contract(&CONTRACT)
        .output(output_schema_for::<Layout>)
        .params("stock?: stock ids; library?: shared stock JSON path (default: metric preset); kerf?: 3.2 mm; trim?: 10 mm per edge; mode?: guillotine; path?: .svg|.dxf|.json|.csv; labels_path?: .svg. Reads cutlist.parts, respects manufacturing grain and separates materials and thicknesses. Reports every unplaced piece; labels include all pieces. Kerf must be positive. Layouts are computed, never saved."),
];

pub fn library(path: Option<&str>) -> Result<StockLibrary> {
    let bytes = path.map(|p| solidcraft_io::vfs::read(p, 1 << 20).map_err(|e| bad(ID, e.to_string()))).transpose()?;
    let library: StockLibrary = serde_json::from_slice(bytes.as_deref().unwrap_or(METRIC.as_bytes())).map_err(|e| bad(ID, e.to_string()))?;
    if library.stock.len() > 128 {
        return Err(bad(ID, "stock library is limited to 128 choices"));
    }
    let mut ids = std::collections::BTreeSet::new();
    for s in &library.stock {
        if s.id.is_empty()
            || !ids.insert(&s.id)
            || [&s.id, &s.name, &s.material].iter().any(|v| v.len() > 1024)
            || !matches!(s.kind.as_str(), "panel" | "board" | "length")
            || [s.length, s.width, s.thickness].iter().any(|v| !v.is_finite() || *v <= 0.0 || *v > 1e7)
            || s.price.is_some_and(|v| !v.is_finite() || !(0.0..=1e12).contains(&v))
        {
            return Err(bad(ID, "stock ids must be unique; kind panel|board|length; dimensions positive and finite; prices finite and nonnegative"));
        }
    }
    Ok(library)
}

fn layout(s: &mut Session, value: &Value) -> Result<Value> {
    let p: LayoutParams = serde_json::from_value(value.clone()).map_err(|e| bad(ID, e.to_string()))?;
    // Validate settings even for an empty cut list.
    solidcraft_nest::pack(&[], &[], Settings { linear: false, kerf: p.kerf, trim: p.trim }).map_err(|e| bad(ID, e))?;
    let library = library(p.library.as_deref())?;
    if let Some(ids) = &p.stock
        && (ids.len() > 128 || ids.iter().any(|id| !library.stock.iter().any(|s| &s.id == id)))
    {
        return Err(bad(ID, "unknown stock id or too many stock choices"));
    }
    if p.path.as_ref().is_some_and(|v| !["svg", "dxf", "json", "csv"].iter().any(|ext| v.to_ascii_lowercase().ends_with(&format!(".{ext}"))))
        || p.labels_path.as_ref().is_some_and(|v| !v.to_ascii_lowercase().ends_with(".svg"))
    {
        return Err(bad(ID, "plan path must end in .svg, .dxf, .json or .csv; labels_path must end in .svg"));
    }
    if p.path.is_some() && p.path == p.labels_path {
        return Err(bad(ID, "plan and labels must use different paths"));
    }
    let list: CutList = serde_json::from_value(super::cutlist::parts(s, &json!({}))?).map_err(|e| bad(ID, e.to_string()))?;
    let mut out = build(&list, &library, p.stock.as_deref(), p.kerf, p.trim)?;
    out.available_stock = library.stock;
    if let Some(path) = &p.path {
        let ext = path.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
        let text = match ext.as_str() {
            "svg" => solidcraft_io::workshop::svg(&plan_pages(&out))?,
            "dxf" => solidcraft_io::workshop::dxf(&plan_pages(&out))?,
            "csv" => plan_csv(&out),
            _ => serde_json::to_string_pretty(&out).map_err(|e| bad(ID, e.to_string()))?,
        };
        write(path, &text)?;
    }
    if let Some(path) = &p.labels_path {
        write(path, &solidcraft_io::workshop::svg(&label_pages(&out))?)?;
    }
    serde_json::to_value(out).map_err(|e| bad(ID, e.to_string()))
}

fn write(path: &str, text: &str) -> Result<()> {
    solidcraft_io::vfs::check(path).map_err(|e| EngineError::Other(e.to_string()))?;
    #[cfg(not(target_arch = "wasm32"))]
    solidcraft_io::safe_write::write_atomic(std::path::Path::new(path), text.as_bytes(), false).map_err(|e| EngineError::Other(e.to_string()))?;
    #[cfg(target_arch = "wasm32")]
    solidcraft_io::vfs::write(path, text.as_bytes()).map_err(|e| EngineError::Other(e.to_string()))?;
    Ok(())
}

fn mark(mut item: usize, piece: u64) -> String {
    let mut letters = Vec::new();
    while item > 0 {
        item -= 1;
        letters.push((b'A' + (item % 26) as u8) as char);
        item /= 26;
    }
    format!("{}-{piece}", letters.into_iter().rev().collect::<String>())
}

pub(super) fn build(list: &CutList, library: &StockLibrary, selected: Option<&[String]>, kerf: f64, trim: f64) -> Result<Layout> {
    if list.total_occurrences > 2048 || list.rows.len() > 2048 {
        return Err(bad(ID, "cutting plans are limited to 2048 physical pieces"));
    }
    let mut out = Layout { excluded: list.excluded.clone(), ..Layout::default() };
    // Material, thickness and cross-section are never mixed on one stock piece.
    let mut groups: BTreeMap<(String, String, String, String), Vec<(Part, usize)>> = BTreeMap::new();
    for row in &list.rows {
        // The cut list's panel ratio also covers 1x4 timber.
        let linear = row.stock.kind != "panel"
            || (row.grain != "width"
                && library.stock.iter().any(|s| {
                    s.kind != "panel"
                        && selected.is_none_or(|ids| ids.contains(&s.id))
                        && (s.material.is_empty() || s.material == row.material)
                        && format!("{:.6}", s.thickness) == format!("{:.6}", row.stock.thickness)
                        && format!("{:.6}", s.width) == format!("{:.6}", row.stock.width)
                        && row.stock.length <= s.length - 2.0 * trim
                }));
        for piece in 1..=row.count {
            if out.labels.len() >= 2048 {
                return Err(bad(ID, "cutting plans are limited to 2048 physical pieces"));
            }
            let label = out.labels.len();
            out.labels.push(Label {
                item: row.item,
                mark: mark(row.item, piece),
                name: row.name.clone(),
                length: row.stock.length,
                width: row.stock.width,
                thickness: row.stock.thickness,
                material: row.material.clone(),
                grain: row.grain.clone(),
            });
            // An approximate stock box (curved router edges) still encloses the part, so it is cut from that box.
            let unsupported = matches!(row.grain.as_str(), "thickness" | "oblique") || (linear && row.grain == "width");
            if unsupported {
                out.unplaced.push(Unplaced { item: row.item, mark: mark(row.item, piece), reason: "grain cannot follow this stock's length".into() });
                continue;
            }
            let rotation = match row.grain.as_str() {
                "" | "none" => Rotation::Free,
                "width" => Rotation::QuarterTurn,
                _ => Rotation::Fixed,
            };
            let part = Part { length: row.stock.length, width: if linear { 1.0 } else { row.stock.width }, rotation };
            groups
                .entry((
                    row.material.clone(),
                    format!("{:.6}", row.stock.thickness),
                    if linear { format!("{:.6}", row.stock.width) } else { String::new() },
                    if linear { "linear" } else { "panel" }.into(),
                ))
                .or_default()
                .push((part, label));
        }
    }
    let mut purchases: BTreeMap<(String, String), Purchase> = BTreeMap::new();
    for ((material, thickness, width, kind), entries) in groups {
        let linear = kind == "linear";
        let choices: Vec<_> = library
            .stock
            .iter()
            .filter(|s| {
                selected.is_none_or(|ids| ids.contains(&s.id))
                    && (s.material.is_empty() || s.material == material)
                    && format!("{:.6}", s.thickness) == thickness
                    && if linear { s.kind != "panel" && format!("{:.6}", s.width) == width } else { s.kind == "panel" }
            })
            .collect();
        let stocks: Vec<_> = choices.iter().map(|s| solidcraft_nest::Stock { length: s.length, width: if linear { 1.0 } else { s.width } }).collect();
        let parts: Vec<_> = entries.iter().map(|(part, _)| part.clone()).collect();
        let plan = solidcraft_nest::pack(&parts, &stocks, Settings { linear, kerf, trim }).map_err(|e| bad(ID, e))?;
        for part in plan.unplaced {
            let (_, index) = entries.get(part).ok_or_else(|| bad(ID, "invalid packing part"))?;
            let label = out.labels.get(*index).ok_or_else(|| bad(ID, "invalid label"))?;
            out.unplaced.push(Unplaced {
                item: label.item,
                mark: label.mark.clone(),
                reason: "no selected stock fits this piece, cross-section, material and grain".into(),
            });
        }
        for sheet in plan.sheets {
            let stock = choices.get(sheet.stock).ok_or_else(|| bad(ID, "invalid packing stock"))?;
            let rectangle = |r: solidcraft_nest::Rect| Rectangle {
                x: r.x,
                y: if linear { 0.0 } else { r.y },
                length: r.length,
                width: if linear { stock.width } else { r.width },
            };
            let mut placements = Vec::new();
            for placement in sheet.placements {
                let (_, index) = entries.get(placement.part).ok_or_else(|| bad(ID, "invalid packing placement"))?;
                let label = out.labels.get(*index).ok_or_else(|| bad(ID, "invalid label"))?;
                let r = rectangle(placement.rect);
                placements.push(Placement {
                    item: label.item,
                    mark: label.mark.clone(),
                    x: r.x,
                    y: r.y,
                    length: r.length,
                    width: r.width,
                    rotated: placement.rotated,
                });
            }
            let purchase = purchases.entry((stock.id.clone(), material.clone())).or_insert_with(|| Purchase {
                stock: stock.id.clone(),
                material: material.clone(),
                count: 0,
                price: stock.price,
            });
            purchase.count += 1;
            out.sheets.push(Sheet {
                stock: stock.id.clone(),
                material: material.clone(),
                length: stock.length,
                width: stock.width,
                thickness: stock.thickness,
                linear,
                placements,
                cuts: sheet.cuts.into_iter().map(rectangle).collect(),
                offcuts: sheet.offcuts.into_iter().map(rectangle).collect(),
                yield_ratio: sheet.yield_ratio,
            });
        }
    }
    out.to_buy = purchases.into_values().collect();
    Ok(out)
}

fn plan_csv(plan: &Layout) -> String {
    let mut rows = vec!["sheet,stock,mark,item,x_mm,y_mm,length_mm,width_mm,rotated".to_string()];
    for (i, sheet) in plan.sheets.iter().enumerate() {
        for p in &sheet.placements {
            rows.push(format!(
                "{},{},{},{},{},{},{},{},{}",
                i + 1,
                solidcraft_doc::csv::quoted(&sheet.stock),
                p.mark,
                p.item,
                p.x,
                p.y,
                p.length,
                p.width,
                p.rotated
            ));
        }
    }
    for p in &plan.unplaced {
        rows.push(format!("unplaced,,{},{},,,,,", p.mark, p.item));
    }
    rows.join("\n") + "\n"
}

pub(super) fn plan_pages(plan: &Layout) -> Vec<Page> {
    plan.sheets
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let mut page = Page { width: s.length, height: s.width + 90.0, ..Page::default() };
            page.texts.push(Text {
                x: 5.0,
                y: 22.0,
                size: 16.0,
                value: format!(
                    "{}: {} / {} — {} x {} x {} mm — {:.1}%",
                    i + 1,
                    s.stock,
                    s.material,
                    s.length,
                    s.width,
                    s.thickness,
                    s.yield_ratio * 100.0
                ),
            });
            page.rectangles.push(SvgRect { x: 0.0, y: 80.0, length: s.length, width: s.width, kind: Kind::Stock });
            for o in &s.offcuts {
                page.rectangles.push(SvgRect { x: o.x, y: o.y + 80.0, length: o.length, width: o.width, kind: Kind::Offcut });
                page.texts.push(Text { x: o.x + 2.0, y: o.y + 91.0, size: 9.0, value: format!("{:.2} x {:.2} mm", o.length, o.width) });
            }
            for p in &s.placements {
                page.rectangles.push(SvgRect { x: p.x, y: p.y + 80.0, length: p.length, width: p.width, kind: Kind::Part });
                page.texts.push(Text { x: p.x + 3.0, y: p.y + 99.0, size: 14.0, value: format!("{} / {:.2} x {:.2} mm", p.mark, p.length, p.width) });
            }
            for (n, c) in s.cuts.iter().enumerate() {
                page.rectangles.push(SvgRect { x: c.x, y: c.y + 80.0, length: c.length, width: c.width, kind: Kind::Kerf });
                page.texts.push(Text { x: c.x + 2.0, y: c.y + 91.0, size: 8.0, value: (n + 1).to_string() });
            }
            page
        })
        .collect()
}

pub(super) fn label_pages(plan: &Layout) -> Vec<Page> {
    plan.labels
        .iter()
        .map(|p| {
            let mut page = Page { width: 100.0, height: 55.0, ..Page::default() };
            page.rectangles.push(SvgRect { x: 0.0, y: 0.0, length: 100.0, width: 55.0, kind: Kind::Stock });
            let grain = match p.grain.as_str() {
                "" | "none" => "",
                "width" => "↑",
                "thickness" => "⊙",
                "oblique" => "↗",
                _ => "→",
            };
            for (y, size, value) in [
                (9.0, 6.0, format!("{} {grain}", p.mark)),
                (18.0, 4.0, p.name.clone()),
                (28.0, 3.5, format!("{:.2} x {:.2} x {:.2} mm", p.length, p.width, p.thickness)),
                (38.0, 3.5, p.material.clone()),
                (48.0, 3.0, p.grain.clone()),
            ] {
                page.texts.push(Text { x: 3.0, y, size, value });
            }
            page
        })
        .collect()
}

#[cfg(test)]
#[path = "cutting_plan_tests.rs"]
mod tests;
