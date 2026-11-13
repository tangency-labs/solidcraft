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
//! block, and moves nothing on it. A logo in a block travels with it, its file brought into the drawing.
//!
//! The built-ins ([`BUILT_INS`]) are made in code when asked for, never stored: ISO A4 to A0, each
//! for a part and for an assembly, one sheet with the ISO border and the built-in title block
//! ([`default_block`](title_block::default_block)), the kind's general tolerances typed over its
//! cell ([`PART_NOTE`], [`ASSEMBLY_NOTE`]). A part's drawing starts from ISO A3 and an assembly's from
//! ISO A2 ([`default_for`]). A drawing saved as a template replaces or changes them: a copy of a
//! built-in with a company's changes, its own note among them, is a template like any other. A later
//! built-in never changes a drawing made from an earlier one: a drawing keeps copies, never names.
//! An older name (`iso-a3`, before the built-ins came in two kinds) reads as that size's part one.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use solidcraft_geom::cell::Part;
use solidcraft_geom::drawing_sheet::{self, DrawingBorderProfile, Orientation, PaperSize};
use solidcraft_geom::revision::RevisionRecord;
use solidcraft_geom::title_block::{self, CellKind, TitleBlock};

use super::Drawing;
use super::image::ImageFile;
use crate::format::DRAWING;
use crate::ids::{self, IdSource};
use crate::{DocError, Document, Result};

/// The built-in templates, by name: each ISO size for a part and for an assembly.
pub const BUILT_INS: &[&str] = &[
    "iso-a4-part",
    "iso-a4-assembly",
    "iso-a3-part",
    "iso-a3-assembly",
    "iso-a2-part",
    "iso-a2-assembly",
    "iso-a1-part",
    "iso-a1-assembly",
    "iso-a0-part",
    "iso-a0-assembly",
];
/// Where every built-in's ids come from, so one built-in is always the same bytes.
const BUILT_IN_SEED: u64 = 3875;
/// The ISO sizes the built-ins come in, as their names and titles write them.
const SIZES: [(PaperSize, &str); 5] =
    [(PaperSize::A4, "A4"), (PaperSize::A3, "A3"), (PaperSize::A2, "A2"), (PaperSize::A1, "A1"), (PaperSize::A0, "A0")];

/// The general tolerances a part's built-ins carry.
pub const PART_NOTE: &str = "Unless stated: linear ±0.2 mm, angles ±0.5°. Break sharp edges 0.2 mm.";
/// The general tolerances an assembly's built-ins carry.
pub const ASSEMBLY_NOTE: &str = "Unless stated: assembled positions ±0.5 mm, angles ±1°. Part tolerances are on the part drawings.";
/// The title block cell the notes are typed over.
const TOLERANCES_CELL: &str = "general_tolerances";

/// The built-in a new drawing starts from when nothing else is chosen: ISO A3 for a part, ISO A2 for
/// an assembly.
pub fn default_for(assembly: bool) -> &'static str {
    if assembly { "iso-a2-assembly" } else { "iso-a3-part" }
}

/// Whether `doc`'s drawings are an assembly's, for the template they start from: it places components.
pub fn is_assembly(doc: &Document) -> bool {
    !doc.occurrences.is_empty()
}

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
        (!template.sheet_images.is_empty(), "images placed on its sheets"),
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
/// dimensions, balloons, placed images, revision rows, and the component each sheet documents),
/// nor its number, which drawings made from it get their own of. Its other properties, its revision
/// numbering, its logos and the text typed over its cells of plain text stay; text typed over a cell
/// that shows a field (a parameter, a value or a reference) goes with the design that field reads.
pub fn from_drawing(drawing: &Drawing) -> Result<Drawing> {
    let mut template = drawing.clone();
    template.sources.clear();
    template.views.clear();
    template.dimensions.clear();
    template.balloons.clear();
    template.sheet_images.clear();
    template.drop_unused_images();
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
pub fn apply(drawing: &Drawing, template: &Drawing, id_source: &mut IdSource) -> Result<(Drawing, Vec<Restyled>)> {
    check(template)?;
    let mut d = drawing.clone();
    let logos = logos_in(&mut d, template, id_source)?;
    let mut changed = Vec::new();
    for at in 0..d.sheets.len() {
        let Some(from) = template.sheets.get(at).or(template.sheets.last()) else { continue };
        let block = from.title_block.as_ref().map(|block| with_logos(block, &logos));
        let Some(sheet) = d.sheets.get_mut(at) else { continue };
        let paper = sheet.sheet != from.sheet;
        let (border, title_block) = (sheet.border != from.border, sheet.title_block != block);
        // A sheet's typed text is always its own block's ([`Drawing::validate`]), so the same block loses none.
        if !(paper || border || title_block) {
            continue;
        }
        let sheet = Arc::make_mut(sheet);
        sheet.sheet = from.sheet;
        sheet.border = from.border;
        let lost = sheet.set_title_block(block);
        changed.push(Restyled { sheet: sheet.id, paper, border, title_block, lost });
    }
    // A logo goes with the block that showed it.
    d.drop_unused_images();
    d.validate()?;
    Ok((d, changed))
}

/// Each logo file `template` shows, brought into `d` once however many of its sheets show it: the
/// template's id for it, to `d`'s. One only a sheet past `d`'s last shows goes with what's unused.
fn logos_in(d: &mut Drawing, template: &Drawing, id_source: &mut IdSource) -> Result<BTreeMap<u64, u64>> {
    let mut brought = BTreeMap::new();
    for logo in template.logos() {
        if let Entry::Vacant(entry) = brought.entry(logo) {
            let file = template.images.by_id(logo).ok_or_else(|| DocError::Invalid(format!("the template has no image {}", ids::encode(logo))))?;
            let id = d.allocate_id(id_source)?;
            entry.insert(d.keep_file(ImageFile { id, ..file.clone() }));
        }
    }
    Ok(brought)
}

/// `block` with each logo cell naming its file's id in the drawing (`logos`, from [`logos_in`]).
fn with_logos(block: &TitleBlock, logos: &BTreeMap<u64, u64>) -> TitleBlock {
    let mut block = block.clone();
    for cell in &mut block.cells {
        if let CellKind::Text(text) = &mut cell.kind {
            text.image = text.image.and_then(|logo| logos.get(&logo).copied());
        }
    }
    block
}

/// The built-in `name`'s paper size, its title ("ISO A3") and whether it is an assembly's; none for
/// another name. An older name without its kind is the part's.
fn parse(name: &str) -> Option<(PaperSize, String, bool)> {
    let rest = name.strip_prefix("iso-")?;
    let (size, assembly) = match rest.split_once('-') {
        None => (rest, false),
        Some((size, "part")) => (size, false),
        Some((size, "assembly")) => (size, true),
        Some(_) => return None,
    };
    let (paper, label) = SIZES.into_iter().find(|(_, label)| label.to_ascii_lowercase() == size)?;
    Some((paper, format!("ISO {label}"), assembly))
}

/// The title of the built-in template `name` ("ISO A3"), without making it; none for another name.
pub fn title(name: &str) -> Option<String> {
    parse(name).map(|(_, title, _)| title)
}

/// Whether the built-in template `name` is an assembly's; none for another name.
pub fn assembly(name: &str) -> Option<bool> {
    parse(name).map(|(.., assembly)| assembly)
}

/// The built-in template `name` ([`BUILT_INS`]), made now; none for another name.
pub fn built_in(name: &str) -> Option<Result<Drawing>> {
    let (size, title, assembly) = parse(name)?;
    // A4 is used upright (ISO 5457); the larger sizes lie flat.
    let orientation = if size == PaperSize::A4 { Orientation::Portrait } else { Orientation::Landscape };
    Some(made(&title, drawing_sheet::Sheet::standard(size, orientation), if assembly { ASSEMBLY_NOTE } else { PART_NOTE }))
}

/// One sheet of `paper` with the ISO border and the built-in title block, `note` typed over its
/// general tolerances.
fn made(title: &str, paper: drawing_sheet::Sheet, note: &str) -> Result<Drawing> {
    let mut id_source = IdSource::seeded(BUILT_IN_SEED);
    let mut d = Drawing::new(title, "en", "Sheet 1", paper, Some(DrawingBorderProfile::iso()), &BTreeSet::new(), &mut id_source)?;
    if let Some(first) = d.sheets.first_mut() {
        let first = Arc::make_mut(first);
        first.title_block = Some(title_block::default_block());
        first.title_block_overrides.insert(TOLERANCES_CELL.into(), note.into());
    }
    check(&d)?;
    Ok(d)
}

#[cfg(test)]
#[path = "template_tests.rs"]
mod tests;
