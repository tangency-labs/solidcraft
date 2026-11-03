//! Drawing templates (#3875): what a new drawing starts from. A template is a drawing with no
//! model: its sheets with their borders, title blocks and the text typed over their cells, its
//! projection, units, language, properties and revision numbering, and nothing that draws or
//! documents a design ([`check`]). So it is saved and read as a drawing is, under the same caps
//! and damage rules, and a new drawing is a copy of it with ids of its own ([`instantiate`]).
//!
//! A drawing saved as a template ([`from_drawing`]) keeps its format and leaves its model behind; a
//! template file reads as a drawing does, then is held to the template rules ([`read`]).
//!
//! A template applied to a drawing ([`apply`]) gives each sheet its paper, drawing border and title
//! block, and moves nothing on it.
//!
//! The built-ins ([`BUILT_INS`]) are made in code when asked for, never stored: ISO A4 to A0, each
//! one sheet with the ISO border and the built-in title block
//! ([`default_block`](title_block::default_block)). A drawing saved as a template replaces or
//! changes them: a copy of a built-in with a company's changes is a template like any other. A
//! later built-in never changes a drawing made from an earlier one: a drawing keeps copies, never
//! names.

use std::collections::BTreeSet;
use std::sync::Arc;

use solidcraft_geom::cell::Part;
use solidcraft_geom::drawing_sheet::{self, DrawingBorderProfile, Orientation, PaperSize};
use solidcraft_geom::revision::RevisionRecord;
use solidcraft_geom::title_block::{self, CellKind};

use super::Drawing;
use crate::format::DRAWING;
use crate::ids::IdSource;
use crate::{DocError, Result};

/// The built-in templates, by name: each ISO size, with the ISO border and the built-in title block.
pub const BUILT_INS: &[&str] = &["iso-a4", "iso-a3", "iso-a2", "iso-a1", "iso-a0"];
/// Where every built-in's ids come from, so one built-in is always the same bytes.
const BUILT_IN_SEED: u64 = 3875;
/// The ISO sizes the built-ins come in, as their names and titles write them.
const SIZES: [(PaperSize, &str); 5] =
    [(PaperSize::A4, "A4"), (PaperSize::A3, "A3"), (PaperSize::A2, "A2"), (PaperSize::A1, "A1"), (PaperSize::A0, "A0")];

/// The built-in a new drawing starts from when nothing else is chosen.
pub const DEFAULT: &str = "iso-a3";

/// The template rules: a drawing's ([`Drawing::validate`]), and nothing of a model. A template
/// with anything of one is refused, naming all it has.
pub fn check(template: &Drawing) -> Result<()> {
    template.validate()?;
    let has = [
        (!template.sources.is_empty(), "a source"),
        (!template.views.is_empty(), "views"),
        (!template.dimensions.is_empty(), "dimensions"),
        (!template.balloons.is_empty(), "balloons"),
        (template.revisions.ids().next().is_some(), "revision rows"),
        (template.sheets.iter().any(|sheet| sheet.documents.is_some()), "a sheet that documents a component"),
    ];
    let has: Vec<&str> = has.into_iter().filter(|(has, _)| *has).map(|(_, what)| what).collect();
    if has.is_empty() {
        return Ok(());
    }
    Err(DocError::Invalid(format!("a template draws no design, and this one has {}", has.join(", "))))
}

/// A new drawing named `name` from `template`: a copy with the format this build writes, an id
/// none of `taken` has (its design's drawings'), and each sheet an id of its own, all from
/// `id_source`.
pub fn instantiate(template: &Drawing, name: &str, taken: &BTreeSet<u64>, id_source: &mut IdSource) -> Result<Drawing> {
    check(template)?;
    let id = id_source.next(taken)?;
    let mut in_use = BTreeSet::from([id]);
    let mut sheets = Vec::with_capacity(template.sheets.len());
    for sheet in &template.sheets {
        let sheet_id = id_source.next(&in_use)?;
        in_use.insert(sheet_id);
        sheets.push(Arc::new(super::Sheet { id: sheet_id, ..(**sheet).clone() }));
    }
    let format = DRAWING.format_string(DRAWING.current());
    let drawing = Drawing { format, format_pending: DRAWING.pending_names(), id, name: name.into(), sheets, ..template.clone() };
    drawing.validate()?;
    Ok(drawing)
}

/// `drawing` as a template: its format, and nothing it draws or documents (its sources, views,
/// dimensions, balloons, revision rows, and the component each sheet documents), nor its number,
/// which drawings made from it get their own of. Its other properties, its revision numbering and
/// the text typed over its cells of plain text stay; text typed over a cell that shows a field
/// (a parameter, a value or a reference) goes with the design that field reads.
pub fn from_drawing(drawing: &Drawing) -> Result<Drawing> {
    let mut template = drawing.clone();
    template.sources.clear();
    template.views.clear();
    template.dimensions.clear();
    template.balloons.clear();
    Arc::make_mut(&mut template.properties).remove(crate::metadata::PART_NUMBER);
    let mut revisions = RevisionRecord::default();
    (revisions.numbering, revisions.per_sheet) = (template.revisions.numbering, template.revisions.per_sheet);
    template.revisions = Arc::new(revisions);
    for sheet in &mut template.sheets {
        let sheet = Arc::make_mut(sheet);
        sheet.documents = None;
        let shows_a_field = |name: &String| {
            let cell = sheet.title_block.as_ref().and_then(|block| block.cells.iter().find(|cell| cell.name == *name));
            cell.is_some_and(
                |cell| matches!(&cell.kind, CellKind::Text(text) if text.content.0.iter().any(|part| matches!(part, Part::Field { .. }))),
            )
        };
        sheet.title_block_overrides.retain(|name, _| !shows_a_field(name));
    }
    check(&template)?;
    Ok(template)
}

/// A template file's text, read as a drawing is ([`Drawing::from_json`]: its size cap, the format
/// check and the damage rules), then held to the template rules.
pub fn read(text: &str) -> Result<Drawing> {
    let template = Drawing::from_json(text)?;
    check(&template)?;
    Ok(template)
}

/// What applying a template changed on one sheet ([`apply`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Restyled {
    pub sheet: u64,
    pub paper: bool,
    pub border: bool,
    pub title_block: bool,
    /// The cells whose typed text is dropped: the new block has no text cell of that name.
    pub lost: Vec<String>,
}

/// `drawing` in `template`'s sheet formats: each sheet takes the paper, drawing border and title
/// block of the template's sheet in its place (its last for the sheets past its own), and keeps the
/// text typed over a cell while the new block has a text cell of that name. Nothing on a sheet
/// moves. Answers the drawing, and what changed on each sheet that did.
pub fn apply(drawing: &Drawing, template: &Drawing) -> Result<(Drawing, Vec<Restyled>)> {
    check(template)?;
    let mut d = drawing.clone();
    let mut changed = Vec::new();
    for (at, sheet) in d.sheets.iter_mut().enumerate() {
        let Some(from) = template.sheets.get(at).or(template.sheets.last()) else { continue };
        let paper = sheet.sheet != from.sheet;
        let (border, title_block) = (sheet.border != from.border, sheet.title_block != from.title_block);
        // A sheet's typed text is always its own block's ([`Drawing::validate`]), so the same block loses none.
        if !(paper || border || title_block) {
            continue;
        }
        let sheet = Arc::make_mut(sheet);
        sheet.sheet = from.sheet;
        sheet.border = from.border;
        let lost = sheet.set_title_block(from.title_block.clone());
        changed.push(Restyled { sheet: sheet.id, paper, border, title_block, lost });
    }
    d.validate()?;
    Ok((d, changed))
}

/// The built-in `name`'s paper size and its title ("ISO A3"); none for another name.
fn parse(name: &str) -> Option<(PaperSize, String)> {
    let size = name.strip_prefix("iso-")?;
    let (size, label) = SIZES.into_iter().find(|(_, label)| label.to_ascii_lowercase() == size)?;
    Some((size, format!("ISO {label}")))
}

/// The title of the built-in template `name` ("ISO A3"), without making it; none for another name.
pub fn title(name: &str) -> Option<String> {
    parse(name).map(|(_, title)| title)
}

/// The built-in template `name` ([`BUILT_INS`]), made now; none for another name.
pub fn built_in(name: &str) -> Option<Result<Drawing>> {
    let (size, title) = parse(name)?;
    // A4 is used upright (ISO 5457); the larger sizes lie flat.
    let orientation = if size == PaperSize::A4 { Orientation::Portrait } else { Orientation::Landscape };
    Some(made(&title, drawing_sheet::Sheet::standard(size, orientation)))
}

/// One sheet of `paper` with the ISO border and the built-in title block.
fn made(title: &str, paper: drawing_sheet::Sheet) -> Result<Drawing> {
    let mut id_source = IdSource::seeded(BUILT_IN_SEED);
    let mut d = Drawing::new(title, "en", "Sheet 1", paper, Some(DrawingBorderProfile::iso()), &BTreeSet::new(), &mut id_source)?;
    if let Some(first) = d.sheets.first_mut() {
        Arc::make_mut(first).title_block = Some(title_block::default_block());
    }
    check(&d)?;
    Ok(d)
}

#[cfg(test)]
#[path = "template_tests.rs"]
mod tests;
