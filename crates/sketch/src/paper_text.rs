//! Text on paper (#4890 B1b): a text laid out by [`TextMetrics`], the rule sketch text draws and
//! measures with, as the paper scene's runs of placed glyphs ([`TextRun`]), with the fonts they
//! are set in ([`Font`]), each holding the glyphs its runs place.
//!
//! - A text is set in the font family its drawing names (#4890 F1): an installed family, found as
//!   sketch text finds one ([`find_style_in`]), or the built-in Liberation Sans for none. A family
//!   that isn't installed, or whose face can't be set on paper, is set in the built-in font, and
//!   [`paper_font_note`] says so. A run names its font by the font's id ([`Font::id`]).
//! - Bold takes the family's bold face, and is refused without one; an italic takes its italic
//!   face, or leans as sketch text's made-up italic does.
//! - A face is admitted once a process, the first time it is asked for (#4890 F2). Any face with
//!   outlines is: a variable one at its default instance (what a PDF draws), with the cap height sketch
//!   text scales it by, and what its tables lack made up for:
//!   - a PostScript name of what a PDF name can hold of its own, else of its family and style, else
//!     `Font-` and the start of its id;
//!   - without a `post` table, no italic angle, and a fixed pitch when every glyph advances alike;
//!   - its stem width measured across its `l` (no table gives it), else from its weight (a PDF reader
//!     uses it only to choose a stand-in, never for an embedded font).
//!
//!   How a PDF may carry it ([`Embed`]) comes from its embedding permissions. A bitmap or colour-only
//!   face, with no outlines to fill, is refused.
//! - A character the font has no glyph for is refused by name: sketch's `?` stand-in is never set
//!   on paper.
//! - Lines break at `\n`, a line pitch apart, and are aligned as sketch text's are. A line with
//!   right-to-left characters is a run per direction, shaped. Every run of a line starts where the
//!   line does, its glyphs placed along the whole line.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use sha2::{Digest, Sha256};
use solidcraft_geom::paper::text::{
    Cluster, Embed, Font, FontFlags, FontGlyph, FontMetrics, MAX_POSTSCRIPT_NAME, PlacedGlyph, TextRun, is_postscript_char,
};
use solidcraft_geom::{Loop2, Vec2};

use crate::fonts::{DEFAULT_FONT_FAMILY, FontData, check_family, find_style_in, font_data, font_dirs, normalize};
use crate::model::{Result, SketchError};
use crate::text::{LaidLine, MAX_TEXT_CHARS, Run, SLANT_DEG, TextMetrics, TextStyle, check_height, check_spacing};

/// Most faces kept admitted at once (each holds its file's bytes); past it the table starts again.
const MAX_ADMITTED: usize = 16;

/// Faces admitted for paper, by file (None: the built-in font) and face index: each one's record
/// with no glyphs yet, or why it can't be set on paper.
type Admitted = HashMap<(Option<PathBuf>, u32), std::result::Result<Font, String>>;
static ADMITTED: OnceLock<Mutex<Admitted>> = OnceLock::new();

/// `text` on paper in the font `family` (None or empty: the built-in font), its first line's
/// baseline starting at `at` (sheet mm) and turned `angle` radians about it, at a capital height of
/// `height` mm, set in `style` (which follows no path on paper). Its runs, and the fonts they use
/// (each once, in order of id, holding the glyphs its runs place); None when it sets no glyph
/// (empty lines only).
pub fn paper_text(
    text: &str,
    family: Option<&str>,
    at: Vec2,
    height: f64,
    angle: f64,
    style: &TextStyle,
) -> Result<Option<(Vec<TextRun>, Vec<Font>)>> {
    paper_text_in(&font_dirs(), text, family, at, height, angle, style)
}

/// [`paper_text`] with the fonts installed in `dirs`.
pub fn paper_text_in(
    dirs: &[PathBuf],
    text: &str,
    family: Option<&str>,
    at: Vec2,
    height: f64,
    angle: f64,
    style: &TextStyle,
) -> Result<Option<(Vec<TextRun>, Vec<Font>)>> {
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
    let (data, mut font, family) = paper_face(dirs, family, style.bold, style.italic)?;
    let metrics = TextMetrics::new(&data, style)?;
    let (k, pitch) = (metrics.scale(height)?, metrics.line_pitch(height)?);
    let slant = if style.italic && !font.metrics.flags.italic { SLANT_DEG.to_radians().tan() } else { 0.0 };
    let (sin, cos) = angle.sin_cos();
    let mut runs = Vec::new();
    for (row, line) in text.split('\n').enumerate() {
        let missing = metrics.missing(line);
        if !missing.is_empty() {
            let chars: String = missing.iter().map(|c| format!(" {c:?}")).collect();
            return Err(SketchError::Invalid(format!("the font {family} has no glyph for{chars}")));
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
    Ok(Some((runs, vec![font])))
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

/// The face a text in `family` is set in on paper, from the fonts of `dirs`, its record, and the
/// family it is: the built-in font for none, or for a family that isn't installed or can't be set on
/// paper ([`paper_font_note`] says which). Bold takes a bold face, and is refused without one.
fn paper_face(dirs: &[PathBuf], family: Option<&str>, bold: bool, italic: bool) -> Result<(FontData, Font, String)> {
    let name = family.map(str::trim).unwrap_or("");
    check_family(name)?;
    let (data, font, face_bold, name) = match installed_face(dirs, name, bold, italic) {
        Some(Ok((data, font, face_bold))) => (data, font, face_bold, name),
        _ => {
            let data = font_data(None)?;
            let font = admitted(&data).map_err(|e| SketchError::Invalid(format!("the built-in font can't be set on paper: {e}")))?;
            (data, font, false, DEFAULT_FONT_FAMILY)
        }
    };
    if bold && !face_bold {
        return Err(SketchError::Invalid(format!("the font {name} has no bold face")));
    }
    Ok((data, font, name.to_string()))
}

/// An installed family's face for paper, from `dirs`, with its record and whether it is bold, or
/// why it can't be used; None for the built-in font (no name, or its own).
fn installed_face(dirs: &[PathBuf], name: &str, bold: bool, italic: bool) -> Option<std::result::Result<(FontData, Font, bool), String>> {
    if name.is_empty() || normalize(name) == normalize(DEFAULT_FONT_FAMILY) {
        return None;
    }
    Some(match find_style_in(dirs, name, bold, italic) {
        Ok(Some((data, face_bold, _))) => {
            admitted(&data).map(|font| (data, font, face_bold)).map_err(|e| format!("{name} can't be set on paper ({e})"))
        }
        Ok(None) => Err(format!("{name} isn't on this computer")),
        Err(e) => Err(format!("{name} can't be read ({e})")),
    })
}

/// Why text in `family` is set in the built-in font instead, from the fonts of `dirs`, for a
/// drawing's status ("Futura isn't on this computer: set in Liberation Sans"); None when the family
/// is set itself.
pub fn paper_font_note(dirs: &[PathBuf], family: &str) -> Option<String> {
    let reason = installed_face(dirs, family.trim(), false, false)?.err()?;
    Some(format!("{reason}: set in {DEFAULT_FONT_FAMILY}"))
}

/// A face's record for paper, admitted once (the module's notes), with no glyphs yet.
fn admitted(data: &FontData) -> std::result::Result<Font, String> {
    let key = (data.path().map(Path::to_path_buf), data.index);
    let mut table = ADMITTED.get_or_init(Default::default).lock().map_err(|_| "the table of fonts is unusable".to_string())?;
    if let Some(admitted) = table.get(&key) {
        return admitted.clone();
    }
    if table.len() >= MAX_ADMITTED {
        table.clear();
    }
    let admitted = admit(data);
    table.insert(key, admitted.clone());
    admitted
}

fn admit(data: &FontData) -> std::result::Result<Font, String> {
    let face = ttf_parser::Face::parse(data.bytes(), data.index).map_err(|e| e.to_string())?;
    let tables = face.tables();
    if tables.glyf.is_none() && tables.cff.is_none() && tables.cff2.is_none() {
        return Err("it has no outlines: a bitmap or colour font".into());
    }
    let id: [u8; 32] = Sha256::new().chain_update(data.bytes()).chain_update(data.index.to_le_bytes()).finalize().into();
    let metrics = TextMetrics::new(data, &TextStyle::default()).map_err(|e| e.to_string())?;
    // At most 70 % of 16384 units per em, or a table's own i16.
    let cap_height = metrics.cap().round() as i16;
    let fixed_pitch = if tables.post.is_some() { face.is_monospaced() } else { same_advances(&face) };
    let b = face.global_bounding_box();
    Ok(Font {
        id,
        bytes: Arc::from(data.bytes()),
        face: data.index,
        postscript_name: postscript_name(&face, &id),
        embed: embed(&face),
        num_glyphs: face.number_of_glyphs(),
        units_per_em: face.units_per_em(),
        metrics: FontMetrics {
            ascent: face.ascender(),
            descent: face.descender(),
            cap_height,
            bbox: [b.x_min, b.y_min, b.x_max, b.y_max],
            italic_angle: f64::from(face.italic_angle()),
            flags: FontFlags { fixed_pitch, serif: serif(&face), italic: face.is_italic(), symbolic: false },
            stem_v: stem(&face, &metrics).unwrap_or_else(|| weight_stem(&face)),
        },
        glyphs: BTreeMap::new(),
    })
}

/// The face's PostScript name as a PDF takes it: what a PDF name can hold of its own, else of its
/// family and style, else `Font-` and the first 4 bytes of its id in hex.
fn postscript_name(face: &ttf_parser::Face, id: &[u8; 32]) -> String {
    use ttf_parser::name_id::{FAMILY, POST_SCRIPT_NAME, SUBFAMILY, TYPOGRAPHIC_FAMILY, TYPOGRAPHIC_SUBFAMILY};
    let name = |ids: &[u16]| ids.iter().find_map(|&want| face.names().into_iter().filter(|n| n.name_id == want).find_map(|n| n.to_string()));
    let held = |s: String| -> Option<String> {
        let kept: String = s.chars().filter(|c| is_postscript_char(*c)).take(MAX_POSTSCRIPT_NAME).collect();
        Some(kept.trim_matches('-').to_string()).filter(|n| !n.is_empty())
    };
    let made = || held(format!("{}-{}", name(&[TYPOGRAPHIC_FAMILY, FAMILY])?, name(&[TYPOGRAPHIC_SUBFAMILY, SUBFAMILY]).unwrap_or_default()));
    let hex: String = id.iter().take(4).map(|b| format!("{b:02X}")).collect();
    name(&[POST_SCRIPT_NAME]).and_then(held).or_else(made).unwrap_or_else(|| format!("Font-{hex}"))
}

/// How a PDF may carry the face: not at all when its licence restricts it (fsType 0x0002) or allows
/// only its bitmaps (0x0200), or its outlines are CFF2, which a PDF can't hold; whole when it may
/// not be subset (0x0100); else a subset.
fn embed(face: &ttf_parser::Face) -> Embed {
    let os2 = face.tables().os2;
    let restricted = os2.is_some_and(|t| matches!(t.permissions(), Some(ttf_parser::Permissions::Restricted)) || !t.is_outline_embedding_allowed());
    if restricted || face.tables().cff2.is_some() {
        Embed::Outlines
    } else if os2.is_some_and(|t| !t.is_subsetting_allowed()) {
        Embed::Whole
    } else {
        Embed::Subset
    }
}

/// Whether every glyph that advances advances alike: a fixed pitch, for a face with no `post`
/// table to say so.
fn same_advances(face: &ttf_parser::Face) -> bool {
    let mut advances = (0..face.number_of_glyphs()).filter_map(|g| face.glyph_hor_advance(ttf_parser::GlyphId(g))).filter(|a| *a > 0);
    advances.next().is_some_and(|first| advances.all(|a| a == first))
}

/// A stem width from the face's weight, for a face with no `l` or `I` to measure: a fifth of its
/// weight class per thousand units of em (80 for a regular weight).
fn weight_stem(face: &ttf_parser::Face) -> u16 {
    let per_em = f64::from(face.weight().to_number()) / 5.0 / 1000.0;
    (per_em * f64::from(face.units_per_em())).round().clamp(1.0, f64::from(u16::MAX)) as u16
}

/// Whether a face has serifs, by its OS/2 family class (the high byte of `sFamilyClass`, at byte
/// 30): classes 1 to 5 and 7 are serif ones.
fn serif(face: &ttf_parser::Face) -> bool {
    let os2 = face.raw_face().table(ttf_parser::Tag::from_bytes(b"OS/2"));
    os2.and_then(|table| table.get(30)).is_some_and(|class| matches!(class, 1..=5 | 7))
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
