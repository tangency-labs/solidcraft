//! Sketch text: glyph outlines of a font as lines, conics and cubic splines, so text closes
//! into profiles that can be extruded. TrueType quadratic Béziers are conics with rho = 0.5;
//! OpenType (CFF) cubic Béziers are cubic control splines, both exact. A glyph's counters (the
//! holes of A, B, 8…) are loops inside its outline; profile detection makes them holes.
//!
//! The default font is embedded (Liberation Sans, SIL OFL 1.1; see `assets/fonts/liberation-sans/OFL.txt` and
//! ATTRIBUTION.md); [`crate::fonts`] finds others by file path or installed family name. [`TextMetrics`] measures
//! text by the rule that draws it. A line with right-to-left characters (Arabic, Hebrew…) is ordered by the
//! Unicode bidirectional algorithm and shaped with the font's own rules, so letters join and marks sit where the font
//! puts them; every other line is set glyph by glyph, as it always was. Text on paper is laid out by the same rule
//! (`paper_text`).

use std::ops::Range;

use serde::{Deserialize, Serialize};
use solidcraft_geom::{Ring, Seg2, Vec2};

use crate::fonts::{FontData, font_data_styled};
use crate::link::{LinkGeom, LinkKind, LinkSource, MAX_LINK_ENTITIES};
use crate::model::{Result, Sketch, SketchError};

/// Most characters in one text.
pub const MAX_TEXT_CHARS: usize = 1000;

/// A glyph's outline as it is walked: its loops of lines, quadratics (conics of weight 1) and cubics, each point
/// placed by `map`.
struct Outline {
    loops: Vec<Vec<Seg2>>,
    start: Vec2,
    cur: Vec2,
    map: Box<dyn Fn(f32, f32) -> Vec2>,
}

impl Outline {
    fn push(&mut self, seg: Seg2) {
        match self.loops.last_mut() {
            Some(open) => open.push(seg),
            None => self.loops.push(vec![seg]),
        }
    }
    fn line(&mut self, to: Vec2) {
        if self.cur.dist(to) > 1e-9 {
            self.push(Seg2::Line { a: self.cur, b: to });
        }
        self.cur = to;
    }
    fn quad(&mut self, apex: Vec2, to: Vec2) {
        let (a, b) = (self.cur, to);
        let chord = b - a;
        let off = chord.cross(apex - a).abs() / chord.len().max(1e-12);
        if off < 1e-6 || a.dist(b) < 1e-9 {
            self.line(to);
            return;
        }
        self.push(Seg2::Conic { a, apex, b, w: 1.0 });
        self.cur = to;
    }
    fn cubic(&mut self, c1: Vec2, c2: Vec2, to: Vec2) {
        let (a, b) = (self.cur, to);
        let chord = b - a;
        let l = chord.len().max(1e-12);
        let off = (chord.cross(c1 - a).abs() / l).max(chord.cross(c2 - a).abs() / l);
        // Straight (controls on the chord, between its ends): a line.
        let between = |c: Vec2| (c - a).dot(chord) >= -1e-9 && (c - b).dot(chord) <= 1e-9;
        if a.dist(b) < 1e-9 || (off < 1e-6 && between(c1) && between(c2)) {
            self.line(to);
            return;
        }
        self.push(Seg2::Cubic { p0: a, p1: c1, p2: c2, p3: b });
        self.cur = to;
    }
}

/// An outline piece as sketch geometry: a quadratic is the conic of rho 0.5, a cubic its control spline. None for
/// the pieces [`Outline`] doesn't make.
fn link_geom(seg: Seg2) -> Option<LinkGeom> {
    match seg {
        Seg2::Line { a, b } => Some(LinkGeom::Line(a, b)),
        Seg2::Conic { a, apex, b, .. } => Some(LinkGeom::Conic { a, apex, b, rho: 0.5 }),
        Seg2::Cubic { p0, p1, p2, p3 } => Some(LinkGeom::Spline { pts: vec![p0, p1, p2, p3], control: true, degree: 3, periodic: false }),
        Seg2::Arc { .. } | Seg2::Bezier { .. } => None,
    }
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = (self.map)(x, y);
        self.loops.push(Vec::new());
        self.start = p;
        self.cur = p;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = (self.map)(x, y);
        self.line(p);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (c, p) = ((self.map)(x1, y1), (self.map)(x, y));
        self.quad(c, p);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (c1, c2, p) = ((self.map)(x1, y1), (self.map)(x2, y2), (self.map)(x, y));
        self.cubic(c1, c2, p);
    }
    fn close(&mut self) {
        let s = self.start;
        self.line(s);
    }
}

/// Where each line of a text sits against its anchor (`at`, or a point of its path).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    /// The line starts at the anchor.
    #[default]
    Left,
    /// The line is centred on it.
    Center,
    /// The line ends at it.
    Right,
}

impl TextAlign {
    fn is_left(&self) -> bool {
        *self == TextAlign::Left
    }

    /// Where a line `width` mm wide starts, along its baseline from the anchor.
    pub(crate) fn start(self, width: f64) -> f64 {
        match self {
            TextAlign::Left => 0.0,
            TextAlign::Center => -width / 2.0,
            TextAlign::Right => -width,
        }
    }
}

/// How a text is set: the style of its font, where its lines sit, the space between its letters
/// and lines, and the sketch curve its baseline follows (instead of the straight line from `at`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "TextAlign::is_left")]
    pub align: TextAlign,
    /// Extra space between letters (mm; negative brings them closer).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub letter_spacing: f64,
    /// The distance between lines as a multiple of the font's own.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub line_spacing: f64,
    /// The id of a curve of the same sketch the baseline follows (`at` and the angle then do not
    /// place the text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Along the path the other way round, so the text sits on its other side.
    #[serde(default, skip_serializing_if = "is_false")]
    pub reverse: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle { bold: false, italic: false, align: TextAlign::Left, letter_spacing: 0.0, line_spacing: 1.0, path: None, reverse: false }
    }
}

impl TextStyle {
    /// Nothing but the defaults (what a text saved before styles had).
    pub fn is_plain(&self) -> bool {
        *self == TextStyle::default()
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

fn one() -> f64 {
    1.0
}

fn is_one(x: &f64) -> bool {
    *x == 1.0
}

/// How far a slanted (made-up italic) glyph leans: 12 degrees.
pub(crate) const SLANT_DEG: f64 = 12.0;
/// Largest letter spacing (mm either way) and line spacing factor accepted.
const MAX_LETTER_SPACING: f64 = 1e5;
const LINE_SPACING_RANGE: (f64, f64) = (0.05, 100.0);

/// A capital height (mm) text can be set at: finite, above a micrometre and below a kilometre.
pub(crate) fn check_height(height: f64) -> Result<()> {
    if height.is_finite() && height > 1e-6 && height < 1e6 { Ok(()) } else { Err(SketchError::Invalid("text height must be positive".into())) }
}

/// A style's spacing: a finite letter spacing of at most [`MAX_LETTER_SPACING`] either way, and a line spacing in
/// [`LINE_SPACING_RANGE`].
pub(crate) fn check_spacing(style: &TextStyle) -> Result<()> {
    if !(style.letter_spacing.is_finite() && style.letter_spacing.abs() <= MAX_LETTER_SPACING) {
        return Err(SketchError::Invalid("the letter spacing must be a finite length".into()));
    }
    if !(style.line_spacing.is_finite() && (LINE_SPACING_RANGE.0..=LINE_SPACING_RANGE.1).contains(&style.line_spacing)) {
        return Err(SketchError::Invalid(format!(
            "the line spacing must be between {} and {} times the font's",
            LINE_SPACING_RANGE.0, LINE_SPACING_RANGE.1
        )));
    }
    Ok(())
}

/// How a font sets text in a style, read once from its face: the scale for a capital height, the distance between
/// lines, the width of a line and the characters the font has no glyph for. Sketch text is laid out with it, so a
/// line measures what it draws. Kerning stays off, as in sketch text.
pub struct TextMetrics<'a> {
    face: ttf_parser::Face<'a>,
    /// The capital height, in font units.
    cap: f64,
    /// The distance between baselines in font units: ascender to descender, plus the line gap (none when not positive).
    pitch: f64,
    /// What a character the font lacks is set as: its `?`, when it has one.
    fallback: Option<ttf_parser::GlyphId>,
    letter_spacing: f64,
    line_spacing: f64,
    /// The font's bytes and face index, for shaping a right-to-left line.
    bytes: &'a [u8],
    index: u32,
    /// What shaping needs from the font, read the first time a right-to-left line asks (None: the font cannot be shaped).
    shaper: std::cell::OnceCell<Option<harfrust::ShaperData>>,
}

/// One glyph of a laid-out line: where its origin is along the line, its advance and how far it is raised (marks),
/// in millimetres, and where in the line the characters it sets start (its cluster), in bytes.
pub(crate) struct Glyph {
    pub(crate) id: ttf_parser::GlyphId,
    pub(crate) x: f64,
    pub(crate) advance: f64,
    pub(crate) rise: f64,
    pub(crate) cluster: usize,
}

/// One line laid out: its glyphs in the order they stand along it, left to right, its runs and its width (mm).
pub(crate) struct LaidLine {
    pub(crate) glyphs: Vec<Glyph>,
    pub(crate) runs: Vec<Run>,
    pub(crate) width: f64,
}

/// A stretch of a line in one direction: its glyphs, and the bytes of the line they set.
pub(crate) struct Run {
    pub(crate) glyphs: Range<usize>,
    pub(crate) text: Range<usize>,
    pub(crate) rtl: bool,
}

impl<'a> TextMetrics<'a> {
    /// The face of `font` set in `style`: its letter and line spacing (its bold and italic chose the face).
    pub fn new(font: &'a FontData, style: &TextStyle) -> Result<TextMetrics<'a>> {
        check_spacing(style)?;
        let mut face = ttf_parser::Face::parse(font.bytes(), font.index).map_err(|e| SketchError::Invalid(format!("font: {e}")))?;
        if let Some(weight) = font.weight {
            face.set_variation(ttf_parser::Tag::from_bytes(b"wght"), weight);
        }
        Ok(TextMetrics {
            cap: cap_height(&face),
            pitch: f64::from(face.ascender()) - f64::from(face.descender()) + f64::from(face.line_gap()),
            fallback: face.glyph_index('?'),
            letter_spacing: style.letter_spacing,
            line_spacing: style.line_spacing,
            bytes: font.bytes(),
            index: font.index,
            shaper: std::cell::OnceCell::new(),
            face,
        })
    }

    /// The capital height text is scaled by, in font units ([`cap_height`]).
    pub(crate) fn cap(&self) -> f64 {
        self.cap
    }

    /// Millimetres per font unit at a capital height of `height` mm.
    pub fn scale(&self, height: f64) -> Result<f64> {
        check_height(height)?;
        Ok(height / self.cap)
    }

    /// The distance between baselines (mm) at a capital height of `height` mm: the font's own (one and a half heights
    /// when it gives none) times the line spacing.
    pub fn line_pitch(&self, height: f64) -> Result<f64> {
        let k = self.scale(height)?;
        let own = if self.pitch > 0.0 { self.pitch * k } else { height * 1.5 };
        Ok(own * self.line_spacing)
    }

    /// The width (mm) of one line at a capital height of `height` mm, as it is drawn: from its first glyph's origin to
    /// the end of its last glyph's advance. A line break in it is a character like any other.
    pub fn line_width(&self, line: &str, height: f64) -> Result<f64> {
        let k = self.scale(height)?;
        Ok(self.laid_out(line, k).width)
    }

    /// The characters of one line the font has no glyph for, in order and each time they come: those its character map
    /// lacks (set as its `?`, or left out when it has none) and those it sends to its missing-glyph box (`.notdef`, drawn
    /// as it is).
    pub fn missing(&self, line: &str) -> Vec<char> {
        line.chars().filter(|ch| self.face.glyph_index(*ch).is_none_or(|g| g.0 == 0)).collect()
    }

    /// The one rule that lays out a line, at `k` mm per font unit: each glyph, where its origin is along the line and
    /// its advance. No kerning; a character the font lacks is set as its `?` (left out when it has none either); the
    /// letter spacing goes between glyphs, not after the last.
    fn glyphs(&self, line: &str, k: f64) -> impl Iterator<Item = Glyph> {
        let mut pen = 0.0f64;
        line.char_indices().filter_map(move |(cluster, ch)| {
            let id = self.face.glyph_index(ch).or(self.fallback)?;
            let advance = f64::from(self.face.glyph_hor_advance(id).unwrap_or(0)) * k;
            let x = pen;
            pen += advance + self.letter_spacing;
            Some(Glyph { id, x, advance, rise: 0.0, cluster })
        })
    }

    /// One line laid out. A line with right-to-left characters is shaped (see [`Self::shaped`]); any other is laid
    /// out by [`Self::glyphs`], as one left-to-right run.
    pub(crate) fn laid_out(&self, line: &str, k: f64) -> LaidLine {
        if let Some(shaped) = self.shaped(line, k) {
            return shaped;
        }
        let glyphs: Vec<Glyph> = self.glyphs(line, k).collect();
        let width = glyphs.last().map(|g| g.x + g.advance).unwrap_or(0.0);
        let runs = vec![Run { glyphs: 0..glyphs.len(), text: 0..line.len(), rtl: false }];
        LaidLine { glyphs, runs, width }
    }

    /// The advance of glyph `id` in font units, from the face's `hmtx`.
    pub(crate) fn advance(&self, id: ttf_parser::GlyphId) -> u16 {
        self.face.glyph_hor_advance(id).unwrap_or(0)
    }

    /// The outline of glyph `id` in font units, as loops, by the rule sketch text draws with.
    pub(crate) fn outline(&self, id: ttf_parser::GlyphId) -> Vec<Vec<Seg2>> {
        let map = Box::new(|x: f32, y: f32| Vec2::new(f64::from(x), f64::from(y)));
        let mut ob = Outline { loops: Vec::new(), start: Vec2::ZERO, cur: Vec2::ZERO, map };
        let _ = self.face.outline_glyph(id, &mut ob);
        ob.loops.retain(|l| !l.is_empty());
        ob.loops
    }

    /// A line with right-to-left characters, set the way the script needs it: the Unicode bidirectional algorithm puts
    /// its runs in display order, each run is shaped by the font's own rules in its own direction (joined letters,
    /// ligatures, marks attached to their letters), and the glyphs come out left to right. None for a line without
    /// right-to-left characters, or when the font cannot be shaped. Kerning stays off, as in the other lines, and the
    /// letter spacing goes after each glyph that has an advance, not after a mark. A character the font lacks is its
    /// missing-glyph box here. Brackets are not mirrored.
    fn shaped(&self, line: &str, k: f64) -> Option<LaidLine> {
        use harfrust::{Direction, Feature, ShapeOptions, Tag, UnicodeBuffer};
        // Nothing below U+0590 is written right to left.
        if line.chars().all(|c| u32::from(c) < 0x0590) {
            return None;
        }
        let info = unicode_bidi::BidiInfo::new(line, None);
        if !info.has_rtl() {
            return None;
        }
        let font = harfrust::FontRef::from_index(self.bytes, self.index).ok()?;
        let data = self.shaper.get_or_init(|| Some(harfrust::ShaperData::new(&font))).as_ref()?;
        let shaper = data.shaper(&font).build();
        let kerning_off = [Feature::new(Tag::new(b"kern"), 0, ..)];
        let (mut out, mut laid, mut pen, mut gap) = (Vec::new(), Vec::new(), 0.0f64, 0.0f64);
        // A line break in the line ends a paragraph of the algorithm; the paragraphs follow one another.
        let runs = info.paragraphs.iter().flat_map(|paragraph| {
            let (levels, runs) = info.visual_runs(paragraph, paragraph.range.clone());
            runs.into_iter().map(move |run| (levels.get(run.start).is_some_and(|l| l.is_rtl()), run))
        });
        for (rtl, run) in runs {
            let first = out.len();
            let mut buffer = UnicodeBuffer::new();
            buffer.push_str(line.get(run.clone())?);
            buffer.set_direction(if rtl { Direction::RightToLeft } else { Direction::LeftToRight });
            buffer.guess_segment_properties();
            let shaped = shaper.shape(buffer, ShapeOptions::new().features(&kerning_off));
            for (glyph, place) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
                let advance = f64::from(place.x_advance) * k;
                out.push(Glyph {
                    id: ttf_parser::GlyphId(u16::try_from(glyph.glyph_id).ok()?),
                    x: pen + f64::from(place.x_offset) * k,
                    advance,
                    rise: f64::from(place.y_offset) * k,
                    // The buffer numbers its clusters by their bytes in the run.
                    cluster: run.start + usize::try_from(glyph.cluster).ok()?,
                });
                pen += advance;
                gap = 0.0;
                if advance != 0.0 {
                    pen += self.letter_spacing;
                    gap = self.letter_spacing;
                }
            }
            laid.push(Run { glyphs: first..out.len(), text: run, rtl });
        }
        Some(LaidLine { glyphs: out, runs: laid, width: pen - gap })
    }
}

/// Outline geometry of `text` in the built-in font (see [`text_geometry_in`]).
pub fn text_geometry(text: &str, at: Vec2, height: f64, angle: f64) -> Result<Vec<LinkGeom>> {
    text_geometry_in(None, text, at, height, angle)
}

/// Outline geometry of `text` in `font` (None: the built-in font; else a font file path or an
/// installed family name, see [`crate::fonts::font_data`]) with its baseline starting at `at`,
/// capital height `height` (mm), rotated by `angle` (radians) about `at`. Lines break at `\n`.
pub fn text_geometry_in(font: Option<&str>, text: &str, at: Vec2, height: f64, angle: f64) -> Result<Vec<LinkGeom>> {
    text_geometry_styled(font, text, at, height, angle, &TextStyle::default(), None)
}

/// Outline geometry of `text` set in `style` (see [`text_geometry_in`]). With `path` (the
/// points of the curve [`TextStyle::path`] names) the baseline follows it: each letter stands on
/// the curve, turned with it, and the lines after the first run below it.
pub fn text_geometry_styled(
    font: Option<&str>,
    text: &str,
    at: Vec2,
    height: f64,
    angle: f64,
    style: &TextStyle,
    path: Option<&[Vec2]>,
) -> Result<Vec<LinkGeom>> {
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(SketchError::TooLarge);
    }
    // The numbers first: finding an installed font can mean reading its folders.
    check_height(height)?;
    if !(angle.is_finite() && at.is_finite()) {
        return Err(SketchError::Invalid("the text's place and angle must be finite".into()));
    }
    check_spacing(style)?;
    let (data, slant) = font_data_styled(font, style.bold, style.italic)?;
    let metrics = TextMetrics::new(&data, style)?;
    let (k, pitch) = (metrics.scale(height)?, metrics.line_pitch(height)?);
    let shear = if slant { SLANT_DEG.to_radians().tan() } else { 0.0 };
    let along = path.map(|pts| Path::new(pts, style.reverse)).transpose()?;
    let anchor = along.as_ref().map(|p| p.anchor(style.align)).unwrap_or(0.0);
    let (sin, cos) = angle.sin_cos();
    let mut out = Vec::new();
    for (row, line) in text.split('\n').enumerate() {
        let base_y = -(row as f64) * pitch;
        let laid = metrics.laid_out(line, k);
        let start = style.align.start(laid.width);
        for Glyph { id: gid, x, advance, rise, .. } in laid.glyphs {
            // The glyph's frame: its origin on the baseline and the way the baseline runs.
            let (origin, dir) = match &along {
                None => {
                    let dir = Vec2::new(cos, sin);
                    (at + dir * (start + x) + Vec2::new(-sin, cos) * base_y, dir)
                }
                Some(p) => {
                    let (mid, dir) = p.frame(anchor + start + x + advance / 2.0, advance);
                    (mid - dir * (advance / 2.0) + Vec2::new(-dir.y, dir.x) * base_y, dir)
                }
            };
            let up = Vec2::new(-dir.y, dir.x);
            let map = Box::new(move |gx: f32, gy: f32| {
                let (gx, gy) = (f64::from(gx) * k, f64::from(gy) * k);
                origin + dir * (gx + gy * shear) + up * (gy + rise)
            });
            let mut ob = Outline { loops: Vec::new(), start: origin, cur: origin, map };
            let _ = metrics.face.outline_glyph(gid, &mut ob);
            out.extend(ob.loops.into_iter().flatten().filter_map(link_geom));
            // A font can give a glyph any number of points: stop where the link would be refused anyway.
            if out.len() > MAX_LINK_ENTITIES {
                return Err(SketchError::TooLarge);
            }
        }
    }
    if out.iter().any(|g| !geom_finite(g)) {
        return Err(SketchError::Invalid("the text does not fit its path".into()));
    }
    Ok(out)
}

fn geom_finite(g: &LinkGeom) -> bool {
    match g {
        LinkGeom::Line(a, b) => a.is_finite() && b.is_finite(),
        LinkGeom::Conic { a, apex, b, rho } => a.is_finite() && apex.is_finite() && b.is_finite() && rho.is_finite(),
        LinkGeom::Spline { pts, .. } => pts.iter().all(|p| p.is_finite()),
        _ => true,
    }
}

/// A curve a text follows, as points with the distance along it to each: oriented so the text
/// reads left to right (an open curve runs rightwards, a closed one clockwise, the text on its
/// outside) unless reversed.
struct Path {
    pts: Vec<Vec2>,
    along: Vec<f64>,
    closed: bool,
}

impl Path {
    fn new(points: &[Vec2], reverse: bool) -> Result<Path> {
        let mut pts: Vec<Vec2> = Vec::with_capacity(points.len());
        for p in points {
            if !p.is_finite() {
                return Err(SketchError::Invalid("the text's path is not a finite curve".into()));
            }
            if pts.last().is_none_or(|q: &Vec2| q.dist(*p) > 1e-9) {
                pts.push(*p);
            }
        }
        let (Some(first), Some(last)) = (pts.first().copied(), pts.last().copied()) else {
            return Err(SketchError::Invalid("the text's path has no length".into()));
        };
        let length: f64 = pts.iter().zip(pts.iter().skip(1)).map(|(a, b)| a.dist(*b)).sum();
        if pts.len() < 2 || length < 1e-9 {
            return Err(SketchError::Invalid("the text's path has no length".into()));
        }
        let closed = first.dist(last) <= 1e-9 * length.max(1.0);
        let readable = if closed {
            // Clockwise: the text stands on the outside.
            Ring::from(pts.clone()).signed_area() <= 0.0
        } else {
            last.x >= first.x - 1e-9 * length
        };
        if readable == reverse {
            pts.reverse();
        }
        let mut along = Vec::with_capacity(pts.len());
        let mut total = 0.0;
        for (i, p) in pts.iter().enumerate() {
            if let Some(prev) = i.checked_sub(1).and_then(|j| pts.get(j)) {
                total += prev.dist(*p);
            }
            along.push(total);
        }
        Ok(Path { pts, along, closed })
    }

    fn length(&self) -> f64 {
        self.along.last().copied().unwrap_or(0.0)
    }

    /// Where the lines are aligned to: an open curve's start, middle or end; on a closed curve
    /// its top, whatever the alignment.
    fn anchor(&self, align: TextAlign) -> f64 {
        if self.closed {
            return self.pts.iter().zip(&self.along).max_by(|(p, _), (q, _)| p.y.total_cmp(&q.y)).map(|(_, s)| *s).unwrap_or(0.0);
        }
        match align {
            TextAlign::Left => 0.0,
            TextAlign::Center => self.length() / 2.0,
            TextAlign::Right => self.length(),
        }
    }

    /// The point at distance `s` along the curve (around a closed one; past the ends of an open
    /// one, straight on).
    fn point(&self, s: f64) -> Vec2 {
        let length = self.length();
        let s = if self.closed { s.rem_euclid(length) } else { s };
        let (first, last) = (self.pts.first().copied().unwrap_or_default(), self.pts.last().copied().unwrap_or_default());
        if !self.closed && s <= 0.0 {
            let d = self.pts.get(1).and_then(|q| (*q - first).normalized()).unwrap_or(Vec2::X);
            return first + d * s;
        }
        if !self.closed && s >= length {
            let n = self.pts.len();
            let d = n.checked_sub(2).and_then(|i| self.pts.get(i)).and_then(|q| (last - *q).normalized()).unwrap_or(Vec2::X);
            return last + d * (s - length);
        }
        let i = self.along.partition_point(|a| *a < s).clamp(1, self.pts.len().saturating_sub(1).max(1));
        match (self.pts.get(i - 1), self.pts.get(i), self.along.get(i - 1), self.along.get(i)) {
            (Some(a), Some(b), Some(sa), Some(sb)) if sb > sa => *a + (*b - *a) * ((s - sa) / (sb - sa)),
            (Some(a), ..) => *a,
            _ => first,
        }
    }

    /// The point at `s` and the way the curve runs there, over a glyph `width` wide (the chord
    /// across it, so a glyph sits evenly on a bend).
    fn frame(&self, s: f64, width: f64) -> (Vec2, Vec2) {
        let h = (width / 2.0).max(self.length() * 1e-4).max(1e-9);
        let dir = (self.point(s + h) - self.point(s - h)).normalized().unwrap_or(Vec2::X);
        (self.point(s), dir)
    }
}

impl Sketch {
    /// The outline geometry of a text source of this sketch: its path, when it has one, is a
    /// curve of this sketch as it is now.
    pub fn text_source_geometry(&self, src: &LinkSource) -> Result<Vec<LinkGeom>> {
        let LinkSource::Text { text, at, height, angle, font, style, .. } = src else {
            return Err(SketchError::Invalid("not a text".into()));
        };
        let path = match &style.path {
            None => None,
            Some(id) => {
                let ci = self.curve_index(id).ok_or_else(|| SketchError::Invalid(format!("the text's path `{id}` is not a curve of this sketch")))?;
                let of_text =
                    self.curves.get(ci).and_then(|c| c.link.as_deref()).and_then(|l| self.link(l)).is_some_and(|l| l.kind == LinkKind::Text);
                if of_text {
                    return Err(SketchError::Invalid("a text cannot follow the outline of a text".into()));
                }
                Some(self.polyline(ci))
            }
        };
        text_geometry_styled(font.as_deref(), text, *at, *height, *angle, style, path.as_deref())
    }
}

/// The font's capital height in font units: from its OS/2 table, else the top of its `H`, else
/// 70 % of the em.
fn cap_height(face: &ttf_parser::Face) -> f64 {
    face.capital_height()
        .map(f64::from)
        .filter(|c| *c > 0.0)
        .or_else(|| face.glyph_index('H').and_then(|g| face.glyph_bounding_box(g)).map(|b| f64::from(b.y_max)).filter(|c| *c > 0.0))
        .unwrap_or(f64::from(face.units_per_em().max(1)) * 0.7)
}

#[cfg(test)]
#[path = "text_style_tests.rs"]
mod style_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_have_closed_outlines_at_the_cap_height() {
        let g = text_geometry("HO", Vec2::new(5.0, 0.0), 10.0, 0.0).unwrap();
        assert!(!g.is_empty());
        let ys: Vec<f64> = g
            .iter()
            .flat_map(|x| match *x {
                LinkGeom::Line(a, b) => vec![a.y, b.y],
                LinkGeom::Conic { a, b, .. } => vec![a.y, b.y],
                _ => vec![],
            })
            .collect();
        let top = ys.iter().cloned().fold(f64::MIN, f64::max);
        assert!((top - 10.0).abs() < 0.3, "{top}");
        assert!(g.iter().any(|x| matches!(x, LinkGeom::Conic { .. })), "O has curves");
        assert!(text_geometry("x", Vec2::ZERO, -1.0, 0.0).is_err());
        assert!(text_geometry(&"x".repeat(2000), Vec2::ZERO, 1.0, 0.0).is_err());
        // Empty text makes nothing (not an error here; the command rejects it).
        assert!(text_geometry("", Vec2::ZERO, 1.0, 0.0).unwrap().is_empty());
    }

    #[test]
    fn a_font_file_gives_the_same_outlines_and_unknown_fonts_are_errors() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts/liberation-sans/LiberationSans-Regular.ttf");
        let by_file = text_geometry_in(Some(&path.to_string_lossy()), "A7_L", Vec2::ZERO, 8.0, 0.0).unwrap();
        assert_eq!(by_file, text_geometry("A7_L", Vec2::ZERO, 8.0, 0.0).unwrap());
        let e = text_geometry_in(Some("No Such Family 12345"), "A", Vec2::ZERO, 8.0, 0.0).unwrap_err().to_string();
        assert!(e.contains("unknown font"), "{e}");
    }

    #[test]
    fn cubic_outlines_stay_exact_splines() {
        let mut ob = Outline { loops: Vec::new(), start: Vec2::ZERO, cur: Vec2::ZERO, map: Box::new(|x, y| Vec2::new(f64::from(x), f64::from(y))) };
        ttf_parser::OutlineBuilder::move_to(&mut ob, 0.0, 0.0);
        ttf_parser::OutlineBuilder::curve_to(&mut ob, 0.0, 10.0, 10.0, 10.0, 10.0, 0.0);
        // A cubic with its controls on the chord is a line.
        ttf_parser::OutlineBuilder::curve_to(&mut ob, 7.0, 0.0, 3.0, 0.0, 0.0, 0.0);
        let out: Vec<LinkGeom> = ob.loops.into_iter().flatten().filter_map(link_geom).collect();
        assert_eq!(out.len(), 2);
        assert!(matches!(&out[0], LinkGeom::Spline { pts, control: true, degree: 3, .. } if pts.len() == 4));
        assert!(matches!(out[1], LinkGeom::Line(..)));
    }
}
