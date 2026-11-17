//! Workshop SVG pages with text. Derived output only; no design-format state.
use std::fmt::Write;

#[derive(Default)]
pub struct Page {
    pub width: f64,
    pub height: f64,
    pub rectangles: Vec<Rectangle>,
    pub texts: Vec<Text>,
}

pub struct Rectangle {
    pub x: f64,
    pub y: f64,
    pub length: f64,
    pub width: f64,
    pub kind: Kind,
}

pub enum Kind {
    Stock,
    Part,
    Offcut,
    Kerf,
}

pub struct Text {
    pub x: f64,
    pub y: f64,
    pub size: f64,
    pub value: String,
}

fn escape(text: &str) -> String {
    text.chars()
        .filter(|c| (*c >= ' ' && !matches!(*c, '\u{fffe}' | '\u{ffff}')) || matches!(*c, '\n' | '\t' | '\r'))
        .collect::<String>()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Stack pages at their physical millimetre size. Each nested SVG is one stock piece or label.
pub fn svg(pages: &[Page]) -> crate::Result<String> {
    let finite = |n: f64| n.is_finite() && (0.0..=1e9).contains(&n);
    if pages.len() > 4096
        || pages
            .iter()
            .any(|p| !finite(p.width) || !finite(p.height) || p.width == 0.0 || p.height == 0.0 || p.rectangles.len() + p.texts.len() > 20_000)
    {
        return Err(crate::IoError::Invalid("invalid workshop page size or record count".into()));
    }
    let width = pages.iter().map(|p| p.width).fold(1.0, f64::max);
    let height = pages.iter().map(|p| p.height + 5.0).sum::<f64>().max(1.0);
    let mut out = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}mm\" height=\"{height}mm\" viewBox=\"0 0 {width} {height}\">\n");
    let mut y = 0.0;
    for (i, page) in pages.iter().enumerate() {
        let _ = writeln!(
            out,
            "<svg id=\"page-{}\" x=\"0\" y=\"{y}\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">",
            i + 1,
            page.width,
            page.height,
            page.width,
            page.height
        );
        for r in &page.rectangles {
            if ![r.x, r.y, r.length, r.width].into_iter().all(finite) {
                return Err(crate::IoError::Invalid("invalid workshop rectangle".into()));
            }
            let (class, fill) = match r.kind {
                Kind::Stock => ("stock", "white"),
                Kind::Part => ("part", "#e3edf4"),
                Kind::Offcut => ("offcut", "#eeeeee"),
                Kind::Kerf => ("kerf", "#333333"),
            };
            let _ = writeln!(
                out,
                "<rect class=\"{class}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{fill}\" stroke=\"black\" stroke-width=\"0.3\"/>",
                r.x, r.y, r.length, r.width
            );
        }
        for t in &page.texts {
            if ![t.x, t.y, t.size].into_iter().all(finite) || t.value.len() > 8192 {
                return Err(crate::IoError::Invalid("invalid workshop text".into()));
            }
            let available = (page.width - t.x - 3.0).max(1.0);
            let fit = if t.value.chars().count() as f64 * t.size * 0.65 > available {
                format!(" textLength=\"{available}\" lengthAdjust=\"spacingAndGlyphs\"")
            } else {
                String::new()
            };
            let _ = writeln!(
                out,
                "<text x=\"{}\" y=\"{}\" font-size=\"{}\" font-family=\"sans-serif\"{fit}>{}</text>",
                t.x,
                t.y,
                t.size,
                escape(&t.value)
            );
        }
        out.push_str("</svg>\n");
        y += page.height + 5.0;
    }
    out.push_str("</svg>\n");
    Ok(out)
}

/// The same stock pages as DXF, one page above the next, using the existing drawing writer.
pub fn dxf(pages: &[Page]) -> crate::Result<String> {
    // Share the SVG writer's boundary checks before reading coordinates.
    let _ = svg(pages)?;
    use crate::dxf_out::{DxfEntity, DxfLayer, DxfShape, write_dxf_drawing_units};
    use solidcore_record::Vec2;
    let mut entities = Vec::new();
    let mut offset = 0.0;
    for page in pages {
        for r in &page.rectangles {
            let layer = match r.kind {
                Kind::Stock => "STOCK",
                Kind::Part => "PARTS",
                Kind::Offcut => "OFFCUTS",
                Kind::Kerf => "KERF",
            };
            entities.push(DxfEntity {
                layer: layer.into(),
                shape: DxfShape::LwPolyline {
                    verts: [(r.x, r.y), (r.x + r.length, r.y), (r.x + r.length, r.y + r.width), (r.x, r.y + r.width)]
                        .map(|(x, y)| (Vec2::new(x, -y - offset), 0.0))
                        .to_vec(),
                    closed: true,
                },
            });
        }
        for t in &page.texts {
            entities.push(DxfEntity {
                layer: "MARKS".into(),
                shape: DxfShape::Text { at: Vec2::new(t.x, -t.y - offset), height: t.size, angle_deg: 0.0, text: t.value.clone() },
            });
        }
        offset += page.height + 5.0;
    }
    // Millimetres: DXF $INSUNITS 4.
    write_dxf_drawing_units(
        &[DxfLayer::new("STOCK", 7), DxfLayer::new("PARTS", 5), DxfLayer::new("OFFCUTS", 8), DxfLayer::new("KERF", 1), DxfLayer::new("MARKS", 7)],
        &entities,
        4,
    )
}

#[cfg(test)]
#[path = "workshop_tests.rs"]
mod tests;
