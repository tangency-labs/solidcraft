//! A title block's text (#3875, #4890 B3): what each text cell shows on a sheet, and the block's
//! labels and values set on paper by the cell text rule ([`solidcraft_geom::cell`]), in the
//! drawing's font and language.
//!
//! - [`shown`] works out each text cell's text: the text typed over it on the sheet, or its content
//!   with each field read ([`Values::text`]). `drawing.inspect` lists it, and the sheet scene sets it.
//! - [`lay`] measures each label as one line at its place, and each value's lines in its value box,
//!   broken between words to the box's width when its fit wraps. What doesn't fit is shown whole
//!   and noted ([`Warning`]). A text the drawing's font can't set is left off the sheet and noted
//!   with why, so one name in a script no font has never fails the sheet or its export. The window's
//!   sheet doesn't wait for the search for fonts to fall back to: a text it is still looking for is
//!   off the sheet until then ([`Laid::pending`]), never noted as refused.
//!   `drawing.inspect` reads the notes; the sheet scene sets what was measured ([`Laid::set`]),
//!   without measuring again.

use solidcore_record::Vec2;
use solidcraft_doc::Document;
use solidcraft_doc::bom::{Mass, Tree};
use solidcraft_doc::drawing::Drawing;
use solidcraft_doc::drawing::fields::{Component, Values};
use solidcraft_geom::cell::{HAlign, Part, Source, Warning, place_lines};
use solidcraft_geom::paper::text::{Font, TextRun};
use solidcraft_geom::title_block::{CellKind, TextCell, TitleBlock, TitleBlockLayout};
use solidcraft_geom::{Aabb2, EPS};
use solidcraft_sketch::{Outcome, PaperFonts, PaperLines, SketchError, TextAlign, TextStyle};

/// A text cell as its sheet shows it: its name, its text, and the value typed text hides.
#[derive(Clone, Debug, PartialEq)]
pub struct CellText {
    pub name: String,
    pub text: String,
    /// The cell's own value, when text typed over it is shown instead.
    pub replaced: Option<String>,
}

/// What a cell's text on paper says of itself: what doesn't fit, or why the text is off the sheet.
#[derive(Clone, Debug, PartialEq)]
pub enum Note {
    Fit(Warning),
    Refused(String),
}

/// A block's text measured in its cells: each label's and value's lines with where the first
/// starts, in cell order (each cell's label, then its value), and each cell's notes by its name.
#[derive(Debug, Default)]
pub struct Laid {
    texts: Vec<(PaperLines, Vec2)>,
    pub notes: Vec<(String, Note)>,
    /// Whether a text is off the sheet only until the search for a font to fall back to answers.
    pub pending: bool,
}

/// A block's text set on paper: its runs in cell order, and the fonts they are set in (a font may be
/// listed more than once; the scene keeps each once).
#[derive(Clone, Debug, Default)]
pub struct Set {
    pub runs: Vec<TextRun>,
    pub fonts: Vec<Font>,
}

impl Set {
    /// The faces it is set in, each once, in order of id: one family name can mean another face once
    /// a font is installed or removed, and that face has another id.
    pub fn faces(&self) -> Vec<[u8; 32]> {
        let ids: std::collections::BTreeSet<[u8; 32]> = self.fonts.iter().map(|font| font.id).collect();
        ids.into_iter().collect()
    }
}

/// The text cells of sheet `at`'s title block as the sheet shows them, in the block's order. The
/// component it documents is weighed in `tree` only when a cell reads its material or mass.
pub fn shown(doc: &Document, tree: &mut Tree<'_>, d: &Drawing, at: usize) -> Vec<CellText> {
    let Some(sheet) = d.sheets.get(at) else { return Vec::new() };
    let Some(block) = &sheet.title_block else { return Vec::new() };
    let texts = || block.cells.iter().filter_map(|cell| if let CellKind::Text(text) = &cell.kind { Some((&cell.name, &text.content)) } else { None });
    let weighs = texts()
        .flat_map(|(_, content)| &content.0)
        .any(|part| matches!(part, Part::Field { source: Source::Component, key, .. } if key == "material" || key == "mass"));
    // What the model says of the component it documents: a component since deleted has nothing.
    let id = sheet.documents.map(|documents| documents.component);
    let (mass, materials) = id.filter(|_| weighs).and_then(|id| tree.component(id).ok()).unzip();
    let mass = mass.and_then(|mass| if let Mass::Grams(grams) = mass { Some(format_mass(grams)) } else { None });
    let material = materials.filter(|m| !m.is_empty()).map(|m| m.join("; "));
    let component = id.and_then(|id| documented(doc, id, material.as_deref(), mass.as_deref()));
    let values = Values { drawing: d, sheet: at, component };
    let cell = |(name, content): (&String, _)| {
        let value = values.text(content);
        match sheet.title_block_overrides.get(name) {
            Some(typed) => CellText { name: name.clone(), text: typed.clone(), replaced: Some(value) },
            None => CellText { name: name.clone(), text: value, replaced: None },
        }
    };
    texts().map(cell).collect()
}

/// Component `id` of the design (0: the design itself) as a title block's fields read it, with
/// what the model says of it; none when the design has no such component.
fn documented<'a>(doc: &'a Document, id: u64, material: Option<&'a str>, mass: Option<&'a str>) -> Option<Component<'a>> {
    if id == 0 {
        return Some(Component { name: &doc.name, metadata: &doc.metadata, properties: None, material, mass });
    }
    let c = doc.components.iter().find(|c| c.id == id)?;
    Some(Component { name: &c.name, metadata: &c.metadata, properties: Some(&c.properties), material, mass })
}

/// A mass as a sheet shows it: kilograms to two places, until #4867 gives drawings their units.
fn format_mass(grams: f64) -> String {
    format!("{:.2} kg", grams / 1000.0)
}

/// `block`'s labels, and its text cells' values as `shown` gives them, measured in the drawing's
/// lettering (`fonts`) where `layout` places them.
pub fn lay(block: &TitleBlock, layout: &TitleBlockLayout, fonts: &PaperFonts, shown: &[CellText]) -> Laid {
    let mut out = Laid::default();
    for (cell, placed) in block.cells.iter().zip(&layout.cells) {
        let mut notes = Vec::new();
        let label = cell.label.as_ref().zip(placed.label_at);
        let label = label.map(|(label, at)| fonts.lines(&label.text, label.cap_mm, None, &TextStyle::default()).map(|lines| (lines, at)));
        let value = match (&cell.kind, shown.iter().find(|shown| shown.name == cell.name)) {
            (CellKind::Text(text), Some(value)) => Some(value_lines(text, placed.value_box, fonts, &value.text, &mut notes)),
            _ => None,
        };
        for text in [label, value].into_iter().flatten() {
            match Outcome::from(text) {
                Outcome::Set(text) => out.texts.push(text),
                Outcome::Pending => out.pending = true,
                Outcome::Refused(why) => notes.push(Note::Refused(why)),
            }
        }
        out.notes.extend(notes.into_iter().map(|note| (cell.name.clone(), note)));
    }
    out
}

/// `value`'s lines in `value_box`, as `fonts` sets them, by the cell text rule, and where the
/// first starts; what doesn't fit is added to `notes`.
fn value_lines(text: &TextCell, value_box: Aabb2, fonts: &PaperFonts, value: &str, notes: &mut Vec<Note>) -> Result<(PaperLines, Vec2), SketchError> {
    let width = value_box.size().x;
    let align = match text.align {
        HAlign::Left => TextAlign::Left,
        HAlign::Centre => TextAlign::Center,
        HAlign::Right => TextAlign::Right,
    };
    let style = TextStyle { align, ..TextStyle::default() };
    let lines = fonts.lines(value, text.cap_mm, text.fit.wrap.then_some(width), &style)?;
    notes.extend(text.fit.warn_chars(value).map(Note::Fit));
    if lines.lines.iter().any(|(_, w)| *w > width + EPS) {
        notes.push(Note::Fit(Warning::Width));
    }
    let (at, height) = place_lines(value_box, lines.lines.len(), text.cap_mm, lines.pitch, text.align, text.fit.wrap)
        .map_err(|e| SketchError::Invalid(e.to_string()))?;
    notes.extend(height.map(Note::Fit));
    Ok((lines, at))
}

impl Laid {
    /// The measured text set on paper, upright. Measuring admitted the font and found every glyph,
    /// so what was measured is set.
    pub fn set(&self) -> Set {
        let mut out = Set::default();
        for (lines, at) in &self.texts {
            if let Ok(Some((runs, fonts))) = lines.set(*at, 0.0) {
                out.runs.extend(runs);
                out.fonts.extend(fonts);
            }
        }
        out
    }
}

#[cfg(test)]
#[path = "title_block_text_tests.rs"]
mod tests;
