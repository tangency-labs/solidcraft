//! Text on paper (#4890 B1b): a text laid out by [`TextMetrics`], the rule sketch text draws and
//! measures with, as the paper scene's runs of placed glyphs ([`TextRun`]), with the drawing font
//! they are set in ([`Font`]) holding the glyphs they place.
//!
//! - Drawings are set in bundled fonts only, never an installed one, so every machine sets a sheet
//!   alike and a PDF can embed the font. A run names its font by the font's id ([`Font::id`]),
//!   hashed once when the font is admitted. Each is one upright regular face: bold is refused,
//!   and an italic leans as sketch text's made-up italic does.
//! - A font is admitted once, the first time it is asked for: a static face (a PDF embeds a face's
//!   default outlines) with a PostScript name, a cap height and a `post` table, its stem width
//!   measured from its `l` (no table gives it). One that lacks any of them is refused.
//! - A character the font has no glyph for is refused by name: sketch's `?` stand-in is never set
//!   on paper.
//! - Lines break at `\n`, a line pitch apart, and are aligned as sketch text's are. A line with
//!   right-to-left characters is a run per direction, shaped. Every run of a line starts where the
//!   line does, its glyphs placed along the whole line.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use sha2::{Digest, Sha256};
use solidcraft_geom::paper::text::{Cluster, Font, FontFlags, FontGlyph, FontMetrics, PlacedGlyph, TextRun};
use solidcraft_geom::{Loop2, Vec2};

use crate::fonts::{DEFAULT_FONT_FAMILY, font_data, normalize};
use crate::model::{Result, SketchError};
use crate::text::{LaidLine, MAX_TEXT_CHARS, Run, SLANT_DEG, TextMetrics, TextStyle, check_height, check_spacing};

/// A bundled font drawings may be set in: its family, and whether it is a serif face (a PDF
/// reader's font flags; no table says so).
pub struct DrawingFont {
    pub family: &'static str,
    pub serif: bool,
}

/// The fonts drawings are set in, by number.
pub const DRAWING_FONTS: &[DrawingFont] = &[DrawingFont { family: DEFAULT_FONT_FAMILY, serif: false }];

/// Each drawing font, admitted the first time it is asked for: its font record with no glyphs yet.
static ADMITTED: [OnceLock<std::result::Result<Font, String>>; DRAWING_FONTS.len()] = [const { OnceLock::new() }; DRAWING_FONTS.len()];

/// `text` on paper in `family` (None: the default drawing font), its first line's baseline
/// starting at `at` (sheet mm) and turned `angle` radians about it, at a capital height of
/// `height` mm, set in `style` (which follows no path on paper). Its runs, and its font with the
/// glyphs they place; None when it sets no glyph (empty lines only).
pub fn paper_text(text: &str, family: Option<&str>, at: Vec2, height: f64, angle: f64, style: &TextStyle) -> Result<Option<(Vec<TextRun>, Font)>> {
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(SketchError::TooLarge);
    }
    check_height(height)?;
    if !(angle.is_finite() && at.is_finite()) {
        return Err(SketchError::Invalid("the text's place and angle must be finite".into()));
    }
    check_spacing(style)?;
    if style.path.is_some() {
        return Err(SketchError::Invalid("text on paper follows no path".into()));
    }
    let (number, family) = drawing_font(family, style.bold)?;
    let mut font = admitted(number)?;
    let data = font_data(Some(family))?;
    let metrics = TextMetrics::new(&data, style)?;
    let (k, pitch) = (metrics.scale(height)?, metrics.line_pitch(height)?);
    let slant = if style.italic && !font.metrics.flags.italic { SLANT_DEG.to_radians().tan() } else { 0.0 };
    let (sin, cos) = angle.sin_cos();
    let mut runs = Vec::new();
    for (row, line) in text.split('\n').enumerate() {
        let missing = metrics.missing(line);
        if !missing.is_empty() {
            let chars: String = missing.iter().map(|c| format!(" {c:?}")).collect();
            return Err(SketchError::Invalid(format!("the drawing font {family} has no glyph for{chars}")));
        }
        let laid = metrics.laid_out(line, k);
        // The line's start: along the baseline by its alignment, then down a pitch a row.
        let (along, down) = (style.align.start(laid.width), row as f64 * pitch);
        let origin = at + Vec2::new(along * cos + down * sin, along * sin - down * cos);
        for run in laid.runs.iter().filter(|run| !run.glyphs.is_empty()) {
            let (glyphs, text, clusters) = run_records(&laid, run, line)?;
            runs.push(TextRun { font: font.id, scale: k, origin, angle, slant, glyphs, text, clusters, rtl: run.rtl });
        }
    }
    if runs.is_empty() {
        return Ok(None);
    }
    for g in runs.iter().flat_map(|r| &r.glyphs) {
        let id = ttf_parser::GlyphId(g.id);
        font.glyphs.entry(g.id).or_insert_with(|| FontGlyph { advance: metrics.advance(id), outline: outline(&metrics, id) });
    }
    Ok(Some((runs, font)))
}

/// One run of a laid-out line as the scene records it: its glyphs, its text, and its clusters, each
/// the glyphs that start at one place in the text and the text up to the next place a glyph starts
/// (or the run's end), so they work out alike forwards and backwards.
fn run_records(laid: &LaidLine, run: &Run, line: &str) -> Result<(Vec<PlacedGlyph>, String, Vec<Cluster>)> {
    let invalid = || SketchError::Invalid("a run of text the paper can't hold".into());
    let glyphs = laid.glyphs.get(run.glyphs.clone()).ok_or_else(invalid)?;
    let text = line.get(run.text.clone()).ok_or_else(invalid)?;
    let mut starts: Vec<usize> = glyphs.iter().map(|g| g.cluster).collect();
    starts.sort_unstable();
    starts.dedup();
    let index = |v: usize| u32::try_from(v).map_err(|_| invalid());
    let mut clusters = Vec::new();
    let mut first = 0;
    for (i, g) in glyphs.iter().enumerate() {
        if glyphs.get(i + 1).is_some_and(|next| next.cluster == g.cluster) {
            continue;
        }
        let end = starts.iter().find(|s| **s > g.cluster).copied().unwrap_or(run.text.end);
        let (t0, t1) = (g.cluster.checked_sub(run.text.start).ok_or_else(invalid)?, end - run.text.start);
        clusters.push(Cluster { glyphs: index(first)?..index(i + 1)?, text: index(t0)?..index(t1)? });
        first = i + 1;
    }
    let placed = glyphs.iter().map(|g| PlacedGlyph { id: g.id.0, x: g.x, rise: g.rise, advance: g.advance }).collect();
    Ok((placed, text.to_string(), clusters))
}

/// The drawing font a text in `family` (None or empty: the default) is set in, by number, and its
/// family as the list names it.
fn drawing_font(family: Option<&str>, bold: bool) -> Result<(u16, &'static str)> {
    let name = family.map(str::trim).filter(|n| !n.is_empty()).unwrap_or(DEFAULT_FONT_FAMILY);
    let Some((number, font)) = DRAWING_FONTS.iter().enumerate().find(|(_, f)| normalize(f.family) == normalize(name)) else {
        let fonts: Vec<&str> = DRAWING_FONTS.iter().map(|f| f.family).collect();
        return Err(SketchError::Invalid(format!("`{name}` is not a drawing font: drawings are set in {}", fonts.join(", "))));
    };
    if bold {
        return Err(SketchError::Invalid(format!("the drawing font {} has no bold face", font.family)));
    }
    Ok((u16::try_from(number).map_err(|_| SketchError::TooLarge)?, font.family))
}

/// Drawing font `number`'s record, admitted once (the module's notes), with no glyphs yet.
fn admitted(number: u16) -> Result<Font> {
    let (Some(font), Some(cell)) = (DRAWING_FONTS.get(usize::from(number)), ADMITTED.get(usize::from(number))) else {
        return Err(SketchError::Invalid(format!("no drawing font {number}")));
    };
    let admitted = cell.get_or_init(|| admit(font).map_err(|e| format!("the drawing font {} can't be used: {e}", font.family)));
    admitted.clone().map_err(SketchError::Invalid)
}

fn admit(font: &DrawingFont) -> std::result::Result<Font, String> {
    let data = font_data(Some(font.family)).map_err(|e| e.to_string())?;
    let face = ttf_parser::Face::parse(data.bytes(), data.index).map_err(|e| e.to_string())?;
    if face.is_variable() {
        return Err("it is a variable face".into());
    }
    let name = face.names().into_iter().filter(|n| n.name_id == ttf_parser::name_id::POST_SCRIPT_NAME).find_map(|n| n.to_string());
    let postscript_name = name.ok_or("it has no PostScript name")?;
    let cap_height = face.capital_height().filter(|c| *c > 0).ok_or("it has no cap height")?;
    if face.tables().post.is_none() {
        return Err("it has no post table".into());
    }
    let stem_v =
        stem(&face, &TextMetrics::new(&data, &TextStyle::default()).map_err(|e| e.to_string())?).ok_or("it has no upright stem to measure")?;
    let b = face.global_bounding_box();
    Ok(Font {
        id: Sha256::new().chain_update(data.bytes()).chain_update(data.index.to_le_bytes()).finalize().into(),
        bytes: Arc::from(data.bytes()),
        face: data.index,
        postscript_name,
        num_glyphs: face.number_of_glyphs(),
        units_per_em: face.units_per_em(),
        metrics: FontMetrics {
            ascent: face.ascender(),
            descent: face.descender(),
            cap_height,
            bbox: [b.x_min, b.y_min, b.x_max, b.y_max],
            italic_angle: f64::from(face.italic_angle()),
            flags: FontFlags { fixed_pitch: face.is_monospaced(), serif: font.serif, italic: face.is_italic(), symbolic: false },
            stem_v,
        },
        glyphs: BTreeMap::new(),
    })
}

/// A glyph's outline as the scene holds it: closed loops, in font units.
fn outline(metrics: &TextMetrics, id: ttf_parser::GlyphId) -> Vec<Loop2> {
    metrics.outline(id).into_iter().map(|segs| Loop2 { segs }).collect()
}

/// The width of the face's upright stems, in font units: across its `l` (or `I`) halfway up, from
/// where the outline is first crossed to where it is crossed next, each piece walked in 16 steps.
fn stem(face: &ttf_parser::Face, metrics: &TextMetrics) -> Option<u16> {
    let id = ['l', 'I'].into_iter().find_map(|c| face.glyph_index(c))?;
    let b = face.glyph_bounding_box(id)?;
    let mid = (f64::from(b.y_min) + f64::from(b.y_max)) / 2.0;
    let mut xs = Vec::new();
    for seg in metrics.outline(id).iter().flatten() {
        let points: Vec<Vec2> = (0..=16).map(|i| seg.point_at(f64::from(i) / 16.0)).collect();
        for pair in points.windows(2) {
            let (p, q) = (pair[0], pair[1]);
            if (p.y < mid) != (q.y < mid) {
                xs.push(p.x + (mid - p.y) / (q.y - p.y) * (q.x - p.x));
            }
        }
    }
    xs.sort_by(f64::total_cmp);
    let width = xs.get(1)? - xs.first()?;
    u16::try_from(width.round() as i64).ok().filter(|w| *w > 0)
}

#[cfg(test)]
#[path = "paper_text_tests.rs"]
mod tests;
