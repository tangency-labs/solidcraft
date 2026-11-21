//! A sheet PDF's text as real text (#20124): each text run is set with `Tj` in an embedded font,
//! so a viewer selects and searches it and `pdftotext` returns it, and it looks as the outlines do.
//!
//! - **The font** is a Type 3 font built from the glyph outlines the scene already holds, in 1000
//!   units to the em: no font file is read, parsed or subset here, so any face the scene can draw
//!   (variable, CFF, a collection) embeds, and nothing is embedded that the scene did not already
//!   draw. A simple font has at most 255 codes, so one scene font is as many PDF fonts as it needs,
//!   a code for each glyph and the text it carries.
//! - **Placement** is one `Tm` for each glyph, read off the run's own placement of its points
//!   ([`TextRun::placement`]), so the layout the scene fixed (kerning, marks, right to left) is
//!   exactly the page's, with no second copy of the rotation here.
//! - **Extraction:** a `ToUnicode` CMap gives each code its cluster's text. A cluster of several
//!   glyphs (a base and its marks) is wrapped in `ActualText`, its later glyphs mapped to nothing;
//!   a run that reads right to left is wrapped whole in `ActualText` of its text in reading order,
//!   since its glyphs stand in visual order.
//! - **A font a PDF may not carry** ([`Embed::Whole`] and [`Embed::Outlines`]) keeps its outlines
//!   filled on the page as before; its text is set beside them invisibly (`3 Tr`) in a font whose
//!   glyphs are empty, so it is searchable and nothing of the face is embedded.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use pdf_writer::{Name, Pdf, Ref};
use solidcore_record::{Seg2, Vec2};
use solidcraft_geom::paper::text::{Embed, Font, FontGlyph, TextRun};
use solidcraft_geom::prelude::*;

use super::{Refs, quadratic_controls};
use crate::{IoError, MAX_DRAWING_BYTES, Result};

/// The most codes a simple font has: 1 to 255.
const MAX_CODES: usize = 255;
/// Glyph space is 1000 units to the em.
const EM: f64 = 1000.0;
/// The most `bfchar` entries one CMap block holds (the CMap syntax's limit).
const BFCHAR_BLOCK: usize = 100;

/// What a text run adds to a page.
pub(super) struct RunText {
    /// The run's text operators, to append to the page's content.
    pub content: String,
    /// Whether the run's outlines are still filled on the page: the font carries none.
    pub outlines: bool,
}

/// One code of a font: a glyph and the text it carries.
struct Code {
    advance: f64,
    text: Option<String>,
    /// The glyph's procedure, filled; none for an invisible font.
    proc: Option<String>,
}

/// One PDF font: up to 255 codes of one scene font.
struct Chunk {
    reference: Ref,
    name: String,
    codes: Vec<Code>,
}

/// The fonts of one document, filled as its runs are set.
#[derive(Default)]
pub(super) struct TextBook {
    chunks: Vec<Chunk>,
    /// A glyph and its text in a scene font, visible or not: its chunk and code.
    codes: BTreeMap<([u8; 32], bool, u16, Option<String>), (usize, u8)>,
    /// The chunk each scene font, visible or not, is filling.
    open: BTreeMap<([u8; 32], bool), usize>,
}

/// The `(name, reference)` of each font a page uses, for its resources.
pub(super) type PageFonts = BTreeMap<usize, ()>;

impl TextBook {
    /// The operators that set `run` in `font`, its fonts noted in `used`.
    pub fn set(&mut self, refs: &mut Refs, font: &Font, run: &TextRun, used: &mut PageFonts) -> Result<RunText> {
        let visible = font.embed == Embed::Subset;
        let k = EM / f64::from(font.units_per_em);
        let upem = f64::from(font.units_per_em);
        let texts = glyph_texts(run);
        let mut out = String::new();
        if run.rtl {
            let _ = writeln!(out, "/Span << /ActualText {} >> BDC", utf16_hex(&run.text));
        }
        out.push_str("BT\n");
        if !visible {
            out.push_str("3 Tr\n");
        }
        let mut current = None;
        let mut cluster_end = 0usize;
        let mut spans = run.clusters.iter().filter(|c| c.glyphs.len() > 1 && !run.rtl).peekable();
        for (i, g) in run.glyphs.iter().enumerate() {
            let text = texts.get(i).cloned().flatten();
            let glyph = font.glyphs.get(&g.id);
            let key = (font.id, visible, g.id, text.clone());
            let (chunk, code) = match self.codes.get(&key) {
                Some(found) => *found,
                None => {
                    let proc = if visible { glyph.map(|glyph| glyph_proc(glyph, k)) } else { None };
                    let advance = glyph.map_or(0.0, |glyph| f64::from(glyph.advance) * k);
                    self.add(refs, &key, Code { advance, text, proc })?
                }
            };
            if current != Some(chunk) {
                let name = self.chunks.get(chunk).map(|c| c.name.as_str()).unwrap_or_default();
                let _ = writeln!(out, "/{name} 1 Tf");
                used.insert(chunk, ());
                current = Some(chunk);
            }
            if i >= cluster_end
                && let Some(c) = spans.next_if(|c| c.glyphs.start as usize == i)
            {
                cluster_end = c.glyphs.end as usize;
                let actual = run.text.get(c.text.start as usize..c.text.end as usize).unwrap_or_default();
                let _ = writeln!(out, "/Span << /ActualText {} >> BDC", utf16_hex(actual));
            }
            // The glyph's matrix, read off the run's own placement of its points: where its origin
            // lands, and where a step along each font axis goes (an em is `upem` font units).
            let place = run.placement(g);
            let at = place(Vec2::new(0.0, 0.0));
            let (right, up) = (place(Vec2::new(upem, 0.0)) - at, place(Vec2::new(0.0, upem)) - at);
            let (a, b, c, d, e, f) = (right.x, right.y, up.x, up.y, at.x, at.y);
            let _ = writeln!(out, "{} {} {} {} {} {} Tm <{code:02X}> Tj", num(a), num(b), num(c), num(d), num(e), num(f));
            if i + 1 == cluster_end {
                out.push_str("EMC\n");
            }
        }
        if !visible {
            out.push_str("0 Tr\n");
        }
        out.push_str("ET\n");
        if run.rtl {
            out.push_str("EMC\n");
        }
        Ok(RunText { content: out, outlines: !visible })
    }

    /// `code` under `key`, in the font's open chunk, or a new one when that is full.
    fn add(&mut self, refs: &mut Refs, key: &([u8; 32], bool, u16, Option<String>), code: Code) -> Result<(usize, u8)> {
        let scene_font = (key.0, key.1);
        let open = self.open.get(&scene_font).copied().filter(|at| self.chunks.get(*at).is_some_and(|c| c.codes.len() < MAX_CODES));
        let at = match open {
            Some(at) => at,
            None => {
                self.chunks.try_reserve(1).map_err(|_| IoError::Invalid("the sheet PDF: cannot allocate its font list".into()))?;
                self.chunks.push(Chunk { reference: refs.next()?, name: format!("TF{}", self.chunks.len()), codes: Vec::new() });
                let at = self.chunks.len().saturating_sub(1);
                self.open.insert(scene_font, at);
                at
            }
        };
        let chunk = self.chunks.get_mut(at).ok_or_else(|| IoError::Invalid("the sheet PDF: a font went missing".into()))?;
        chunk.codes.try_reserve(1).map_err(|_| IoError::Invalid("the sheet PDF: cannot allocate its font".into()))?;
        chunk.codes.push(code);
        let number = u8::try_from(chunk.codes.len()).map_err(|_| IoError::Invalid("the sheet PDF: a font has too many codes".into()))?;
        self.codes.insert(key.clone(), (at, number));
        Ok((at, number))
    }

    /// The resource name and reference of font `at`.
    pub fn font(&self, at: usize) -> Option<(&str, Ref)> {
        self.chunks.get(at).map(|c| (c.name.as_str(), c.reference))
    }

    /// Writes every font: its dictionary, glyph procedures and `ToUnicode`.
    pub fn write(self, pdf: &mut Pdf, refs: &mut Refs) -> Result<()> {
        let mut bytes = 0usize;
        for chunk in &self.chunks {
            let unicode = refs.next()?;
            let mut procs = Vec::with_capacity(chunk.codes.len());
            for (i, code) in chunk.codes.iter().enumerate() {
                let reference = refs.next()?;
                let body = code.proc.clone().unwrap_or_else(|| format!("{} 0 0 0 0 0 d1\n", num(code.advance)));
                bytes = bytes.saturating_add(body.len());
                pdf.stream(reference, body.as_bytes());
                procs.push((format!("g{}", i + 1), reference));
            }
            let cmap = to_unicode(chunk);
            bytes = bytes.saturating_add(cmap.len());
            if bytes > MAX_DRAWING_BYTES {
                return Err(IoError::Invalid(format!("the PDF's text would be over {MAX_DRAWING_BYTES} bytes")));
            }
            pdf.stream(unicode, cmap.as_bytes());
            let last = i32::try_from(chunk.codes.len()).map_err(|_| IoError::Invalid("the sheet PDF: a font has too many codes".into()))?;
            let mut font = pdf.indirect(chunk.reference).dict();
            font.pair(Name(b"Type"), Name(b"Font"));
            font.pair(Name(b"Subtype"), Name(b"Type3"));
            font.insert(Name(b"FontBBox")).array().items([-1000.0_f32, -1000.0, 2000.0, 2000.0]);
            font.insert(Name(b"FontMatrix")).array().items([0.001_f32, 0.0, 0.0, 0.001, 0.0, 0.0]);
            {
                let mut dict = font.insert(Name(b"CharProcs")).dict();
                for (name, reference) in &procs {
                    dict.pair(Name(name.as_bytes()), *reference);
                }
            }
            {
                let mut encoding = font.insert(Name(b"Encoding")).dict();
                encoding.pair(Name(b"Type"), Name(b"Encoding"));
                let mut differences = encoding.insert(Name(b"Differences")).array();
                differences.item(1_i32);
                for (name, _) in &procs {
                    differences.item(Name(name.as_bytes()));
                }
            }
            font.pair(Name(b"FirstChar"), 1_i32);
            font.pair(Name(b"LastChar"), last);
            font.insert(Name(b"Widths")).array().items(chunk.codes.iter().map(|c| (c.advance * 100.0).round() as f32 / 100.0));
            font.pair(Name(b"ToUnicode"), unicode);
        }
        Ok(())
    }
}

/// Each glyph's text: its cluster's, for a cluster of one glyph or the first of several; none for a
/// later glyph of a cluster, which `ActualText` covers.
fn glyph_texts(run: &TextRun) -> Vec<Option<String>> {
    let mut texts = vec![None; run.glyphs.len()];
    for c in &run.clusters {
        let text = run.text.get(c.text.start as usize..c.text.end as usize).map(str::to_string);
        if let Some(slot) = texts.get_mut(c.glyphs.start as usize) {
            *slot = text;
        }
    }
    texts
}

/// `text` as a PDF text string: UTF-16BE with its byte order mark, in hex.
fn utf16_hex(text: &str) -> String {
    let mut out = String::from("<FEFF");
    for unit in text.encode_utf16() {
        let _ = write!(out, "{unit:04X}");
    }
    out.push('>');
    out
}

/// The font's `ToUnicode` CMap: each code to its text.
fn to_unicode(chunk: &Chunk) -> String {
    let mut out = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<00> <FF>\nendcodespacerange\n",
    );
    let entries: Vec<(usize, &Code)> = chunk.codes.iter().enumerate().map(|(i, c)| (i + 1, c)).collect();
    for block in entries.chunks(BFCHAR_BLOCK) {
        let _ = writeln!(out, "{} beginbfchar", block.len());
        for (code, c) in block {
            let text = c.text.as_deref().unwrap_or_default();
            let hex = utf16_hex(text);
            let _ = writeln!(out, "<{code:02X}> <{}>", hex.trim_start_matches("<FEFF").trim_end_matches('>'));
        }
        out.push_str("endbfchar\n");
    }
    out.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    out
}

/// A number with up to six decimals, no exponent, no "-0".
fn num(x: f64) -> String {
    if !x.is_finite() {
        return "0".into();
    }
    let mut s = format!("{x:.6}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    if s == "-0" || s.is_empty() { "0".into() } else { s }
}

/// A glyph's procedure: its advance and box, then its outline filled nonzero, in 1000 units to the em
/// (`k` of font units).
fn glyph_proc(glyph: &FontGlyph, k: f64) -> String {
    let at = |p: Vec2| (p.x * k, p.y * k);
    let mut body = String::new();
    let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
    let mut note = |(x, y): (f64, f64)| {
        lo = (lo.0.min(x), lo.1.min(y));
        hi = (hi.0.max(x), hi.1.max(y));
    };
    for l in &glyph.outline {
        let Some(first) = l.segs.first() else { continue };
        let (x, y) = at(first.start());
        note((x, y));
        let _ = writeln!(body, "{} {} m", num(x), num(y));
        for seg in &l.segs {
            match *seg {
                Seg2::Cubic { p1, p2, p3, .. } => {
                    let (c1, c2, to) = (at(p1), at(p2), at(p3));
                    [c1, c2, to].into_iter().for_each(&mut note);
                    let _ = writeln!(body, "{} {} {} {} {} {} c", num(c1.0), num(c1.1), num(c2.0), num(c2.1), num(to.0), num(to.1));
                }
                // A parabola is a cubic ([`quadratic_controls`]).
                Seg2::Conic { a, apex, b, w } if (w - 1.0).abs() <= 1e-12 => {
                    let (c1, c2) = quadratic_controls(a, apex, b);
                    let (c1, c2, to) = (at(c1), at(c2), at(b));
                    [c1, c2, to].into_iter().for_each(&mut note);
                    let _ = writeln!(body, "{} {} {} {} {} {} c", num(c1.0), num(c1.1), num(c2.0), num(c2.1), num(to.0), num(to.1));
                }
                // Lines are all a glyph holds besides these (`validate` refuses arcs).
                _ => {
                    let to = at(seg.end());
                    note(to);
                    let _ = writeln!(body, "{} {} l", num(to.0), num(to.1));
                }
            }
        }
        body.push_str("h\n");
    }
    let bounds = if lo.0 <= hi.0 { [lo.0.floor(), lo.1.floor(), hi.0.ceil(), hi.1.ceil()] } else { [0.0; 4] };
    let mut proc =
        format!("{} 0 {} {} {} {} d1\n", num(f64::from(glyph.advance) * k), num(bounds[0]), num(bounds[1]), num(bounds[2]), num(bounds[3]));
    if !body.is_empty() {
        proc.push_str(&body);
        proc.push_str("f\n");
    }
    proc
}
