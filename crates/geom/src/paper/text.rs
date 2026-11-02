//! Text in the paper scene (#4890 B1): runs of placed glyphs, laid out once by sketch's text rule,
//! and the table of the fonts they use. Writers never measure, shape or parse a font: SVG, PNG and
//! DXF fill each glyph's outline ([`TextRun::outlines`]), and PDF embeds the font and places its
//! glyph ids.
//!
//! - A run names its font by the font's id (a hash of its face), never by a place in one scene's
//!   table, so groups laid out apart (each view's, cached) name a font alike whichever machine's
//!   font it is, and [`Scene::add_fonts`] joins their tables when they are assembled.
//! - A font's bytes never go through serde: every `drawing.scene` reply would carry the whole font,
//!   and a scene read from JSON could hand a writer a hostile one. A scene read from JSON has none,
//!   so it is refused until each font's bytes come from the font of its id (sketch, which has
//!   only the faces it has read).
//! - Glyph outlines are filled nonzero: a font's contours may overlap, where the scene's own fills
//!   are even-odd.
//! - Each font says how a PDF may carry it ([`Embed`]): a subset, the whole face, or not at all,
//!   when the PDF draws its glyphs' outlines with the run's text beside them instead.
//!
//! [`Scene::validate`] holds the text to these rules too:
//! - fonts listed by id, each once and each used by a run; its bytes there, at most
//!   [`MAX_FONT_BYTES`]; a PostScript name a PDF takes; glyphs, units per em and a cap height;
//! - a font's glyph table holds exactly the glyphs its runs place, each below its glyph count,
//!   each outline closed loops of lines and Bézier pieces, at most [`MAX_GLYPH_PIECES`] in all;
//! - a run's font in the table; 1 to [`MAX_RUN_GLYPHS`] glyphs and at most [`MAX_RUN_TEXT_BYTES`]
//!   of text; a cap height sketch text could be set at; every number finite, its start and each
//!   glyph's place within the paper's coordinates;
//! - a run's clusters take up its glyphs in order, and its text in order (backwards when it reads
//!   right to left), each with a glyph and whole characters, together covering both once.
//!
//! Its fonts and its runs' glyphs and clusters count toward the scene's records.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::TAU;
use std::ops::Range;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{Budget, Invalid, MAX_COORDINATE_MM, Result, Scene, pieces, require};
use crate::{EPS, Loop2, Seg2, Vec2};

/// The most glyphs a run may place: sketch text's 1000 characters, four glyphs each.
pub const MAX_RUN_GLYPHS: usize = 4000;
/// The most bytes of text a run may carry: 1000 characters of up to four bytes.
pub const MAX_RUN_TEXT_BYTES: usize = 4000;
/// The largest font file, as sketch reads one.
pub const MAX_FONT_BYTES: usize = 64 << 20;
/// The most pieces one glyph's outline may have.
pub const MAX_GLYPH_PIECES: usize = 4096;
/// The longest name a PDF takes.
pub const MAX_POSTSCRIPT_NAME: usize = 127;
/// The cap heights sketch text may be set at, in millimetres: above a micrometre, below a
/// kilometre.
const CAP_HEIGHT_MM: (f64, f64) = (1e-6, 1e6);

/// One font a scene uses, once.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Font {
    /// Its identity, what a run names it by: a hash of its bytes and face, the same for the same face.
    pub id: [u8; 32],
    /// The font file, shared. Never written or read with the scene (the module's notes).
    #[serde(skip)]
    pub bytes: Arc<[u8]>,
    /// The face in the file.
    pub face: u32,
    /// What a PDF names it by ([`is_postscript_char`]).
    pub postscript_name: String,
    /// How a PDF may carry it.
    pub embed: Embed,
    /// How many glyphs the face has: every glyph id is below it.
    pub num_glyphs: u16,
    pub units_per_em: u16,
    pub metrics: FontMetrics,
    /// The glyphs the scene's runs place, by id, and no others.
    pub glyphs: BTreeMap<u16, FontGlyph>,
}

/// How a PDF may carry a font, from its embedding permissions (OS/2 `fsType`) and the kind of its
/// outlines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Embed {
    /// The glyphs its runs place, as a subset of the face.
    #[default]
    Subset,
    /// The whole face: it may not be subset.
    Whole,
    /// Not at all (its licence restricts it, or a PDF can't hold its outlines): the PDF draws the
    /// runs' outlines ([`TextRun::outlines`]) with their text beside them.
    Outlines,
}

/// A character a PDF's font name may hold: printable ASCII, none of `()<>[]{}/%`.
pub fn is_postscript_char(c: char) -> bool {
    c.is_ascii_graphic() && !"()<>[]{}/%".contains(c)
}

/// What a PDF's font descriptor needs, read from the face's own tables (head, hhea, OS/2, post),
/// in font units.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontMetrics {
    pub ascent: i16,
    pub descent: i16,
    /// Above 0: a run's cap height is checked through it.
    pub cap_height: i16,
    /// `[x_min, y_min, x_max, y_max]`.
    pub bbox: [i16; 4],
    /// Degrees anticlockwise from upright, as `post` gives it.
    pub italic_angle: f64,
    pub flags: FontFlags,
    /// The width of the face's upright stems.
    pub stem_v: u16,
}

/// The font descriptor's flags a PDF reader needs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontFlags {
    pub fixed_pitch: bool,
    pub serif: bool,
    pub italic: bool,
    pub symbolic: bool,
}

/// One glyph a scene uses, in font units: its advance width from the face's `hmtx`, and its outline,
/// both read from the bytes a PDF embeds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontGlyph {
    pub advance: u16,
    /// Closed loops of lines and Bézier pieces, filled nonzero.
    pub outline: Vec<Loop2>,
}

/// One run of text in one font and one direction, laid out: writers place it as it is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextRun {
    /// Its font's id ([`Font::id`]).
    pub font: [u8; 32],
    /// Millimetres per font unit.
    pub scale: f64,
    /// Where the baseline starts, in sheet millimetres.
    pub origin: Vec2,
    /// The baseline's direction, in radians anticlockwise from +x.
    pub angle: f64,
    /// A made-up italic's lean: how far a point moves along the baseline per unit up (0 upright).
    pub slant: f64,
    /// In the order they stand along the baseline, from its start.
    pub glyphs: Vec<PlacedGlyph>,
    /// The run's text, in reading order.
    pub text: String,
    /// Which glyphs carry which text, in glyph order.
    pub clusters: Vec<Cluster>,
    /// The run reads right to left: its text runs backwards along its glyphs.
    pub rtl: bool,
}

/// A glyph on its run's baseline, in millimetres.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacedGlyph {
    pub id: u16,
    /// Along the baseline from the run's start.
    pub x: f64,
    /// Above the baseline (a mark's).
    pub rise: f64,
    pub advance: f64,
}

/// The glyphs that carry the text's bytes `text`: many characters to one glyph (a ligature), one
/// character to many glyphs (a base and its marks).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cluster {
    pub glyphs: Range<u32>,
    pub text: Range<u32>,
}

/// A scene's text against the module's rules, `fonts`' records counted into `budget` first. Each font
/// is checked before the runs that read it, so a font's fault is named as the font's.
pub(super) fn check<'a>(fonts: &[Font], runs: impl Iterator<Item = &'a TextRun>, budget: &mut Budget) -> Result<()> {
    for font in fonts {
        budget.add(font.records())?;
        font.check()?;
    }
    require(fonts.iter().zip(fonts.iter().skip(1)).all(|(a, b)| a.id < b.id), "fonts are listed by id, each once")?;
    let mut used = vec![BTreeSet::new(); fonts.len()];
    for run in runs {
        let at = fonts.binary_search_by_key(&run.font, |f| f.id).ok();
        let (Some(font), Some(used)) = at.map_or((None, None), |at| (fonts.get(at), used.get_mut(at))) else {
            return Err(Invalid("a run's font is in the scene's table"));
        };
        run.check(font)?;
        used.extend(run.glyphs.iter().map(|g| g.id));
    }
    for (font, used) in fonts.iter().zip(&used) {
        require(!used.is_empty(), "every font in the table is used by a run")?;
        require(font.glyphs.keys().eq(used.iter()), "a font's table holds exactly the glyphs its runs place")?;
    }
    Ok(())
}

impl Font {
    /// The records the font counts toward the scene's: itself, and each glyph with its loops and
    /// their pieces. Saturates rather than overflows.
    fn records(&self) -> usize {
        let glyph = |g: &FontGlyph| g.outline.iter().fold(1usize, |n, l| n.saturating_add(1).saturating_add(l.segs.len()));
        self.glyphs.values().fold(1usize, |n, g| n.saturating_add(glyph(g)))
    }

    fn check(&self) -> Result<()> {
        const NAMES: &str = "a font's PostScript name is 1 to 127 printable ASCII characters, none of ()<>[]{}/%";
        let bytes = !self.bytes.is_empty() && self.bytes.len() <= MAX_FONT_BYTES;
        require(bytes, "a font has its bytes, at most 64 MiB: a scene read from JSON takes them from the font of its id")?;
        let name = &self.postscript_name;
        require((1..=MAX_POSTSCRIPT_NAME).contains(&name.len()) && name.chars().all(is_postscript_char), NAMES)?;
        require(self.num_glyphs > 0 && (16..=16384).contains(&self.units_per_em), "a font has glyphs, and 16 to 16384 units per em")?;
        require(self.metrics.cap_height > 0 && self.metrics.italic_angle.is_finite(), "a font's cap height is above 0 and its italic angle finite")?;
        for (id, glyph) in &self.glyphs {
            require(*id < self.num_glyphs, "every glyph id is below its font's glyph count")?;
            let count = glyph.outline.iter().map(|l| l.segs.len()).fold(0usize, usize::saturating_add);
            require(count <= MAX_GLYPH_PIECES, "a glyph's outline has at most 4096 pieces")?;
            for l in &glyph.outline {
                require(!l.segs.iter().any(|s| matches!(s, Seg2::Arc { .. })), "a glyph's outline is lines and Bézier pieces")?;
                pieces(&l.segs)?;
                require(!l.segs.is_empty() && l.is_closed(EPS), "each loop of a glyph's outline is closed")?;
            }
        }
        Ok(())
    }
}

impl TextRun {
    /// The records the run counts toward the scene's, beside its item: its glyphs and clusters.
    pub(super) fn records(&self) -> usize {
        self.glyphs.len().saturating_add(self.clusters.len())
    }

    /// Each glyph's outline on the sheet, in millimetres, for the writers that fill text (SVG, PNG,
    /// DXF): scaled, leaned by `slant`, moved along the baseline by its `x` and up by its `rise`, then
    /// turned by `angle` about `origin`. Fill them nonzero (the module's notes). `font` is the font
    /// the run names; a glyph it doesn't hold draws nothing, which `validate` rules out.
    pub fn outlines<'a>(&'a self, font: &'a Font) -> impl Iterator<Item = Loop2> + 'a {
        let turn = Vec2::from_angle(self.angle);
        self.glyphs.iter().filter_map(move |g| Some((g, font.glyphs.get(&g.id)?))).flat_map(move |(g, glyph)| {
            let place = move |p: Vec2| {
                let along = (p.x + self.slant * p.y) * self.scale + g.x;
                let up = p.y * self.scale + g.rise;
                self.origin + Vec2::new(along * turn.x - up * turn.y, along * turn.y + up * turn.x)
            };
            glyph.outline.iter().map(move |l| Loop2 { segs: l.segs.iter().map(|s| s.map_controls(place)).collect() })
        })
    }

    fn check(&self, font: &Font) -> Result<()> {
        let sizes = (1..=MAX_RUN_GLYPHS).contains(&self.glyphs.len()) && self.text.len() <= MAX_RUN_TEXT_BYTES;
        require(sizes, "a run has 1 to 4000 glyphs and at most 4000 bytes of text")?;
        let cap = self.scale * f64::from(font.metrics.cap_height);
        require(self.scale.is_finite() && cap > CAP_HEIGHT_MM.0 && cap < CAP_HEIGHT_MM.1, "a run's cap height is above 1e-6 mm and below 1e6 mm")?;
        let within = |v: f64| v.is_finite() && v.abs() <= MAX_COORDINATE_MM;
        require(within(self.origin.x) && within(self.origin.y), "a run starts within the paper's coordinates")?;
        let lean = self.angle.is_finite() && self.angle.abs() <= TAU && self.slant.is_finite() && self.slant.abs() <= 1.0;
        require(lean, "a run turns at most a full turn either way and leans at most 1")?;
        for g in &self.glyphs {
            require(g.id < font.num_glyphs, "every glyph id is below its font's glyph count")?;
            require([g.x, g.rise, g.advance].into_iter().all(within), "a glyph's place and advance are within the paper's coordinates")?;
        }
        self.check_clusters()
    }

    /// In glyph order, each cluster takes up where the one before ended, in the glyphs and in the
    /// text: forwards, or backwards from the end when the run reads right to left.
    fn check_clusters(&self) -> Result<()> {
        let mut glyph = 0usize;
        let mut text = if self.rtl { self.text.len() } else { 0 };
        for c in &self.clusters {
            let (g0, g1, t0, t1) = (c.glyphs.start as usize, c.glyphs.end as usize, c.text.start as usize, c.text.end as usize);
            require(g0 == glyph && g0 < g1 && t0 < t1, "a run's clusters take up its glyphs in order, each with a glyph and some text")?;
            require(
                if self.rtl { t1 == text } else { t0 == text },
                "a run's clusters take up its text in order, backwards when it reads right to left",
            )?;
            require(self.text.is_char_boundary(t0) && self.text.is_char_boundary(t1), "a cluster's text is whole characters")?;
            glyph = g1;
            text = if self.rtl { t0 } else { t1 };
        }
        let end = if self.rtl { 0 } else { self.text.len() };
        require(glyph == self.glyphs.len() && text == end, "a run's clusters cover all its glyphs and all its text")
    }
}

impl Scene {
    /// Adds `more` to the scene's font table, kept in order of id: a font already there adds its
    /// glyphs to that font's, so groups laid out apart share one table.
    pub fn add_fonts(&mut self, more: impl IntoIterator<Item = Font>) {
        for font in more {
            match self.fonts.binary_search_by_key(&font.id, |f| f.id) {
                Ok(at) => {
                    if let Some(have) = self.fonts.get_mut(at) {
                        have.glyphs.extend(font.glyphs);
                    }
                }
                Err(at) => self.fonts.insert(at, font),
            }
        }
    }
}

#[cfg(test)]
#[path = "text_tests.rs"]
mod tests;
