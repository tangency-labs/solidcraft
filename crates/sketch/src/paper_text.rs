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
//! - A character the font has no glyph for falls back, per character, to an installed font that
//!   has it ([`fallback`]): the same em size as the text's own font, on its baseline and pitch. The
//!   runs of a line split by direction, then by font. A space, a digit or punctuation stays with the
//!   run it follows when that font has it; a mark stays with its letter. A font fallen back to is
//!   shaped, with the drawing's language, and takes its nearest style: bold or italic isn't refused
//!   for it. A character no font has is refused by name: sketch's `?` stand-in is never set on paper.
//! - The search for those fonts reads whole files for the known ones and hashes the ones it finds,
//!   so the window never waits for it: [`paper_text_now`] asks a thread (`font-search`) and refuses
//!   with [`SketchError::FontPending`] until the answers are in. Answers, misses too, are kept for
//!   the process. [`paper_text`] does the search where it is called.
//! - The browser has no font folders: there the web covers Japanese for now ([`fallback`]). Its text
//!   is refused with [`SketchError::FontPending`], whichever way it's asked for, until the font the web
//!   app fetches comes.
//! - Lines break at `\n`, a line pitch apart, and are aligned as sketch text's are. A line with
//!   right-to-left characters is a run per direction, shaped. Every run of a line starts where the
//!   line does, its glyphs placed along the whole line.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use sha2::{Digest, Sha256};
use solidcraft_geom::paper::text::{
    Cluster, Embed, Font, FontFlags, FontGlyph, FontMetrics, MAX_POSTSCRIPT_NAME, PlacedGlyph, TextRun, is_postscript_char,
};
use solidcraft_geom::{Loop2, Vec2};

use crate::fonts::fallback::{self, Han, Query};
use crate::fonts::{DEFAULT_FONT_FAMILY, FontData, check_family, face_at, face_family, find_style_in, font_data, font_dirs, normalize};
use crate::model::{Result, SketchError};
use crate::text::{LaidLine, Laying, MAX_TEXT_CHARS, Run, SLANT_DEG, TextMetrics, TextStyle, check_height, check_spacing};

/// Most faces kept admitted at once (each holds its file's bytes); past it the table starts again.
const MAX_ADMITTED: usize = 16;

/// Faces admitted for paper, by file (None: the built-in font) and face index: each one's record
/// with no glyphs yet, or why it can't be set on paper.
type Admitted = HashMap<(Option<PathBuf>, u32), std::result::Result<Font, String>>;
static ADMITTED: OnceLock<Mutex<Admitted>> = OnceLock::new();

/// `text` on paper in the font `family` (None or empty: the built-in font), in the drawing's
/// `language`, its first line's baseline starting at `at` (sheet mm) and turned `angle` radians about
/// it, at a capital height of `height` mm, set in `style` (which follows no path on paper). Its runs,
/// and the fonts they use (each once, in order of id, holding the glyphs its runs place); None when
/// it sets no glyph (empty lines only). Fonts to fall back to are searched for here.
pub fn paper_text(
    text: &str,
    family: Option<&str>,
    language: &str,
    at: Vec2,
    height: f64,
    angle: f64,
    style: &TextStyle,
) -> Result<Option<(Vec<TextRun>, Vec<Font>)>> {
    paper_text_in(&font_dirs(), text, family, language, at, height, angle, style)
}

/// [`paper_text`] with the fonts installed in `dirs`.
#[allow(clippy::too_many_arguments)]
pub fn paper_text_in(
    dirs: &[PathBuf],
    text: &str,
    family: Option<&str>,
    language: &str,
    at: Vec2,
    height: f64,
    angle: f64,
    style: &TextStyle,
) -> Result<Option<(Vec<TextRun>, Vec<Font>)>> {
    set_text(dirs, text, family, language, (at, height, angle), style, true)
}

/// [`paper_text_in`] for the window's thread: the search for fonts to fall back to runs on a thread
/// of its own, and this refuses with [`SketchError::FontPending`] until its answers are in.
#[allow(clippy::too_many_arguments)]
pub fn paper_text_now(
    dirs: &[PathBuf],
    text: &str,
    family: Option<&str>,
    language: &str,
    at: Vec2,
    height: f64,
    angle: f64,
    style: &TextStyle,
) -> Result<Option<(Vec<TextRun>, Vec<Font>)>> {
    set_text(dirs, text, family, language, (at, height, angle), style, false)
}

fn set_text(
    dirs: &[PathBuf],
    text: &str,
    family: Option<&str>,
    language: &str,
    (at, height, angle): (Vec2, f64, f64),
    style: &TextStyle,
    wait: bool,
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
    let (data, font, family) = paper_face(dirs, family, style.bold, style.italic)?;
    let metrics = TextMetrics::new(&data, style)?;
    let (k, pitch) = (metrics.scale(height)?, metrics.line_pitch(height)?);
    // The fonts this text falls back to, and which sets each character its own font lacks.
    let lacking: BTreeSet<char> = text.split('\n').flat_map(|line| metrics.missing(line)).collect();
    let (others, found) = fallback_faces(dirs, language, text, &lacking, style, wait)?;
    let mut faces = vec![&metrics];
    let other_metrics = others.iter().map(|(data, _)| TextMetrics::new(data, style)).collect::<Result<Vec<_>>>()?;
    faces.extend(&other_metrics);
    let mut records: Vec<Font> = std::iter::once(font).chain(others.iter().map(|(_, font)| font.clone())).collect();
    // Every font takes the same em size.
    let own_em = f64::from(records[0].units_per_em);
    let scales: Vec<f64> = records.iter().map(|font| k * own_em / f64::from(font.units_per_em)).collect();
    let (sin, cos) = angle.sin_cos();
    let lean = SLANT_DEG.to_radians().tan();
    let mut placed: Vec<(usize, TextRun)> = Vec::new();
    for (row, line) in text.split('\n').enumerate() {
        let laid = if metrics.missing(line).is_empty() {
            metrics.laid_out(line, k)
        } else {
            mixed_line(line, &faces, &found, &scales, language, &family, dirs.is_empty())?
        };
        // The line's start: along the baseline by its alignment, then down a pitch a row.
        let (along, down) = (style.align.start(laid.width), row as f64 * pitch);
        let origin = at + Vec2::new(along * cos + down * sin, along * sin - down * cos);
        for run in laid.runs.iter().filter(|run| !run.glyphs.is_empty()) {
            let (glyphs, text, clusters) = run_records(&laid, run, line)?;
            let record = records.get(run.face).ok_or_else(|| SketchError::Invalid("a run in a font it wasn't given".into()))?;
            let slant = if style.italic && !record.metrics.flags.italic { lean } else { 0.0 };
            let run_record = TextRun { font: record.id, scale: scales[run.face], origin, angle, slant, glyphs, text, clusters, rtl: run.rtl };
            placed.push((run.face, run_record));
        }
    }
    if placed.is_empty() {
        return Ok(None);
    }
    for (face, run) in &placed {
        let (record, metrics) = (&mut records[*face], faces[*face]);
        for g in &run.glyphs {
            let id = ttf_parser::GlyphId(g.id);
            record.glyphs.entry(g.id).or_insert_with(|| FontGlyph { advance: metrics.advance(id), outline: outline(metrics, id) });
        }
    }
    // Each font once, in order of id; only those a run uses.
    let mut fonts = BTreeMap::new();
    for record in records.into_iter().filter(|record| !record.glyphs.is_empty()) {
        fonts.entry(record.id).or_insert(record);
    }
    let runs = placed.into_iter().map(|(_, run)| run).collect();
    Ok(Some((runs, fonts.into_values().collect())))
}

/// The fonts `lacking` falls back to, each with its record, and the font (by its place among
/// them, counting the text's own as 0) that sets each character; a character no font has isn't
/// there. Refused with [`SketchError::FontPending`] while they are being looked for, unless `wait`.
fn fallback_faces(
    dirs: &[PathBuf],
    language: &str,
    text: &str,
    lacking: &BTreeSet<char>,
    style: &TextStyle,
    wait: bool,
) -> Result<(Vec<(FontData, Font)>, HashMap<char, usize>)> {
    let (mut others, mut found) = (Vec::new(), HashMap::new());
    if lacking.is_empty() {
        return Ok((others, found));
    }
    let han = Han::of(language, text);
    let queries: Vec<Query> = lacking.iter().map(|ch| Query::new(dirs, han, style.bold, style.italic, *ch)).collect();
    let answers = fallback::answers(&queries, wait).ok_or(SketchError::FontPending)?;
    let mut places: HashMap<(PathBuf, u32), usize> = HashMap::new();
    for (ch, answer) in lacking.iter().zip(answers) {
        let Some(key) = answer else { continue };
        let place = match places.get(&key) {
            Some(place) => *place,
            None => {
                // A face that can't be set on paper sets nothing: its characters are refused by name.
                let Some(face) = face_at(&key.0, key.1).and_then(|data| admitted(&data).ok().map(|font| (data, font))) else { continue };
                others.push(face);
                places.insert(key, others.len());
                others.len()
            }
        };
        found.insert(*ch, place);
    }
    Ok((others, found))
}

/// A line some of whose characters its own font lacks: its runs split by direction, then by font.
fn mixed_line(
    line: &str,
    faces: &[&TextMetrics],
    found: &HashMap<char, usize>,
    scales: &[f64],
    language: &str,
    family: &str,
    on_the_web: bool,
) -> Result<LaidLine> {
    let invalid = || SketchError::Invalid("a run of text the font can't shape".into());
    let spans = font_spans(line, faces, found).map_err(|missing| {
        let chars: String = missing.iter().map(|c| format!(" {c:?}")).collect();
        let web = if on_the_web { ": the web covers Japanese for now" } else { "" };
        SketchError::Invalid(format!("the font {family} has no glyph for{chars}, and no other font here has one{web}"))
    })?;
    let info = unicode_bidi::BidiInfo::new(line, None);
    let mut laying = Laying::default();
    for paragraph in &info.paragraphs {
        let (levels, level_runs) = info.visual_runs(paragraph, paragraph.range.clone());
        for level_run in level_runs {
            let rtl = levels.get(level_run.start).is_some_and(|level| level.is_rtl());
            let cut = |(span, face): &(std::ops::Range<usize>, usize)| {
                let (start, end) = (span.start.max(level_run.start), span.end.min(level_run.end));
                (start < end).then_some((start..end, *face))
            };
            let mut pieces: Vec<(std::ops::Range<usize>, usize)> = spans.iter().filter_map(cut).collect();
            if rtl {
                pieces.reverse();
            }
            for (piece, face) in pieces {
                let (metrics, k) = (*faces.get(face).ok_or_else(invalid)?, *scales.get(face).ok_or_else(invalid)?);
                // Runs in the text's own font stay as sketch text sets them.
                let set = match (face, rtl) {
                    (0, false) => metrics.plain_run(line, piece, k, face, &mut laying),
                    (0, true) => metrics.shape_run(line, piece, true, None, k, face, &mut laying),
                    _ => metrics.shape_run(line, piece, rtl, Some(language), k, face, &mut laying),
                };
                set.ok_or_else(invalid)?;
            }
        }
    }
    Ok(laying.line())
}

/// Which font sets each stretch of `line`, in the line's order, or the characters no font has.
fn font_spans(
    line: &str,
    faces: &[&TextMetrics],
    found: &HashMap<char, usize>,
) -> std::result::Result<Vec<(std::ops::Range<usize>, usize)>, Vec<char>> {
    use unicode_bidi::BidiClass::{AL, BN, L, NSM, R};
    let has = |face: usize, ch: char| faces.get(face).is_some_and(|metrics| metrics.missing(ch.encode_utf8(&mut [0; 4])).is_empty());
    let own = |ch: char| if has(0, ch) { Some(0) } else { found.get(&ch).copied() };
    let (mut spans, mut missing): (Vec<(std::ops::Range<usize>, usize)>, Vec<char>) = (Vec::new(), Vec::new());
    for (at, ch) in line.char_indices() {
        let current = spans.last().map(|(_, face)| *face);
        let face = match unicode_bidi::bidi_class(ch) {
            L | R | AL => own(ch),
            // A mark never leaves its letter's font; a joiner is drawn by none, so one a shaper reads may lack it.
            NSM => current.map_or_else(|| own(ch), |face| has(face, ch).then_some(face)),
            BN => current.filter(|face| *face != 0 || has(0, ch)).or_else(|| own(ch)),
            _ => current.filter(|face| has(*face, ch)).or_else(|| own(ch)),
        };
        let Some(face) = face else {
            missing.push(ch);
            continue;
        };
        match spans.last_mut() {
            Some((span, last)) if *last == face && span.end == at => span.end = at + ch.len_utf8(),
            _ => spans.push((at..at + ch.len_utf8(), face)),
        }
    }
    if missing.is_empty() { Ok(spans) } else { Err(missing) }
}

/// What `texts` fall back to, for a drawing's status ("Hebrew set in Liberation Sans", "Han set in
/// Yu Gothic for Japanese"), sorted, from the answers the search has given: this looks for no font.
pub fn paper_fallback_notes<'t>(dirs: &[PathBuf], family: &str, language: &str, texts: impl IntoIterator<Item = &'t str>) -> Vec<String> {
    let Ok((data, _, _)) = paper_face(dirs, Some(family), false, false) else { return Vec::new() };
    let Ok(metrics) = TextMetrics::new(&data, &TextStyle::default()) else { return Vec::new() };
    let mut notes = BTreeSet::new();
    for text in texts {
        let han = Han::of(language, text);
        for ch in text.split('\n').flat_map(|line| metrics.missing(line)) {
            let Some((path, index)) = fallback::answered(&Query::new(dirs, han, false, false, ch)) else { continue };
            let Some(name) = face_at(&path, index).as_ref().and_then(face_family) else { continue };
            notes.insert(match fallback::script_name(ch, han) {
                (script, Some(language)) => format!("{script} set in {name} for {language}"),
                (script, None) => format!("{script} set in {name}"),
            });
        }
    }
    notes.into_iter().collect()
}

/// Admits the face `index` of the file at `path`, the search's last step, so the window finds it done.
pub(crate) fn warm(path: &Path, index: u32) {
    if let Some(data) = face_at(path, index) {
        let _ = admitted(&data);
    }
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
