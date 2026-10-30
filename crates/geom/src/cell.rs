//! Cell content: what a cell shows, as text and fields, how it should fit, and the cell text rule
//! that places a label and a value in a cell. These are the rules every table of cells shares:
//! title blocks and sheet tables such as parts lists, whose cells and columns are named alike
//! ([`is_snake_case`]). Plain data in sheet millimetres, like
//! [`crate::drawing_sheet`]: nothing here reads a field's value, measures text or draws.
//!
//! - [`Content`] is a list of [`Part`]s: text as written, or a field naming a value by its
//!   [`Source`] and key, with a [`Format`]. Keys, units and decimals are kept as written; the
//!   resolver checks them against each source. [`Content::validate`] bounds its size, and
//!   [`check_label`] a label's.
//! - A value is never cut: when it has more than its [`Fit`] allows, or a word is wider than its
//!   box, it is shown whole with a [`Warning`].
//!
//! The cell text rule (x to the right, y up), in the cell inside its lines, so the padding is
//! measured from a line's inner edge whatever the line's width:
//! - a label's baseline starts [`PADDING_MM`] in from the cell's left side, and its cap height
//!   plus [`PADDING_MM`] down from the cell's top ([`label_at`]);
//! - the value box is the cell inset by [`PADDING_MM`], its top [`PADDING_MM`] below the label's
//!   baseline when there is a label ([`value_box`]);
//! - one line of a value that doesn't wrap is centred top to bottom in the value box; otherwise the
//!   lines run down from the box's top at the font's line pitch, the first baseline a cap height
//!   below it ([`place_lines`]). Each baseline stays in the box, and its descenders
//!   ([`DESCENDER_OF_CAP`]) may reach into the padding under it but never past it to a line;
//! - a line's anchor is on its baseline, at the box's left side, middle or right side: a writer
//!   aligns the line on it.
//!
//! Breaking a value into lines that fit the box's width, and warning about width, need the text
//! measured: they come with the paper scene's text.

use serde::{Deserialize, Serialize};

use crate::paper::MAX_COORDINATE_MM;
use crate::{Aabb2, EPS, Vec2};

/// The most parts a cell's content may have.
pub const MAX_PARTS: usize = 64;
/// The most bytes of text a cell's content may hold in all, and a label.
pub const MAX_TEXT_BYTES: usize = 4096;
/// The longest name a cell or a column may have.
const MAX_NAME_BYTES: usize = 64;
/// What a cell shows for a field without a value: a row field its row doesn't have, a property
/// nobody gave.
pub const MISSING: &str = "-";
/// The space between the inner edge of a cell's lines and its text, in millimetres.
pub const PADDING_MM: f64 = 1.0;
/// How far a line's descenders reach below its baseline, as a share of its cap height: ISO 3098's
/// lettering B, whose lower-case tails are 3/10 of the capitals' height. They may reach into the
/// padding under a value box, never past it.
pub const DESCENDER_OF_CAP: f64 = 0.3;

/// Where a field's value comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The component the sheet documents.
    Component,
    /// The drawing.
    Drawing,
    /// The sheet the cell is on.
    Sheet,
    /// The drawing's newest revision.
    Revision,
    /// An answer given when the block was placed.
    Prompt,
    /// The row of a sheet table the cell is on: a parts list's bill of materials row. A title
    /// block has no rows.
    Row,
}

/// One part of a cell's content. Externally tagged: `{"text": "Sheet "}`,
/// `{"field": {"source": "sheet", "key": "number"}}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Part {
    /// Text shown as written.
    Text(String),
    /// The value `source` holds under `key` (such as `component` `mass`), shown by `format`.
    Field {
        source: Source,
        key: String,
        #[serde(default)]
        format: Format,
    },
}

/// How a field's number is shown; what isn't given comes from the caller's settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Format {
    pub unit: Option<String>,
    pub decimals: Option<u8>,
}

/// What a cell shows: its parts in order.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Content(pub Vec<Part>);

/// What a value should fit beyond its box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fit {
    /// The most characters a value should have, line breaks not counted.
    pub max_chars: Option<u32>,
    /// The most for a value with a wide character (Chinese, Japanese, Korean or Yi), when it
    /// differs from `max_chars`.
    pub wide_max_chars: Option<u32>,
    /// The value breaks between words to fit the box's width, its lines running down from the top.
    #[serde(default)]
    pub wrap: bool,
}

/// Where a line sits across its box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HAlign {
    #[default]
    Left,
    Centre,
    Right,
}

/// The projection a drawing's views use, first or third angle (ISO 5456-2): the value of
/// `drawing.projection`, which the views and the title block's symbol both read. Third angle when
/// nothing chooses (a template or a preference may).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Projection {
    First,
    #[default]
    Third,
}

/// What doesn't fit, when a value is shown whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Warning {
    /// More characters than the fit allows.
    Chars { count: usize, max: u32 },
    /// More lines than the value box holds.
    Height,
    /// A word wider than the value box.
    Width,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("cell: {0}")]
pub struct Invalid(pub &'static str);

type Result<T> = std::result::Result<T, Invalid>;

fn require(condition: bool, message: &'static str) -> Result<()> {
    if condition { Ok(()) } else { Err(Invalid(message)) }
}

/// A lowercase letter, then lowercase letters, digits and underscores: at most 64 in all. Cells and
/// columns are named by it.
pub(crate) fn is_snake_case(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= MAX_NAME_BYTES
        && bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Checks text by the module's rule: at most [`MAX_TEXT_BYTES`] in all, and no control character
/// but a line break.
pub fn check_text<'a>(texts: impl IntoIterator<Item = &'a str>) -> Result<()> {
    let mut bytes = 0usize;
    for text in texts {
        bytes = bytes.saturating_add(text.len());
        require(bytes <= MAX_TEXT_BYTES, "a cell's text is at most 4096 bytes")?;
        require(!text.chars().any(|c| c.is_control() && c != '\n'), "a cell's text has no control character but a line break")?;
    }
    Ok(())
}

impl Part {
    /// Its text, or its key and unit.
    fn texts(&self) -> impl Iterator<Item = &str> {
        let (first, unit) = match self {
            Part::Text(text) => (text.as_str(), None),
            Part::Field { key, format, .. } => (key.as_str(), format.unit.as_deref()),
        };
        std::iter::once(first).chain(unit)
    }
}

impl Content {
    /// Checks the content's size: at most [`MAX_PARTS`] parts, counted first, and its text, keys
    /// and units by the module's text rule.
    pub fn validate(&self) -> Result<()> {
        require(self.0.len() <= MAX_PARTS, "a cell has at most 64 parts")?;
        check_text(self.0.iter().flat_map(Part::texts))
    }
}

/// Checks a label's text: one line of at most [`MAX_TEXT_BYTES`], without control characters.
pub fn check_label(text: &str) -> Result<()> {
    require(text.len() <= MAX_TEXT_BYTES && !text.chars().any(char::is_control), "a label is one line of at most 4096 bytes")
}

impl Fit {
    /// [`Warning::Chars`] when `text` has more characters than the fit allows. A text with a wide
    /// character is held to `wide_max_chars` when it is given.
    pub fn warn_chars(&self, text: &str) -> Option<Warning> {
        let wide = text.chars().any(is_wide);
        let max = if wide { self.wide_max_chars.or(self.max_chars) } else { self.max_chars }?;
        let count = text.chars().filter(|&c| c != '\n').count();
        (count > max as usize).then_some(Warning::Chars { count, max })
    }
}

/// The Unicode blocks of the scripts written in wide characters (Chinese, Japanese, Korean and Yi),
/// with their full-width and vertical forms.
const WIDE: &[(u32, u32)] = &[
    (0x1100, 0x115F),
    (0x2E80, 0x303E),
    (0x3041, 0x33FF),
    (0x3400, 0x4DBF),
    (0x4E00, 0x9FFF),
    (0xA000, 0xA4CF),
    (0xA960, 0xA97F),
    (0xAC00, 0xD7A3),
    (0xF900, 0xFAFF),
    (0xFE10, 0xFE19),
    (0xFE30, 0xFE6F),
    (0xFF00, 0xFF60),
    (0xFFE0, 0xFFE6),
    (0x20000, 0x2FFFD),
    (0x30000, 0x3FFFD),
];

fn is_wide(c: char) -> bool {
    let c = u32::from(c);
    WIDE.iter().any(|&(first, last)| (first..=last).contains(&c))
}

/// Where a label `cap_mm` high goes in `cell` (the cell inside its lines): its baseline's left end.
pub fn label_at(cell: Aabb2, cap_mm: f64) -> Vec2 {
    Vec2::new(cell.min.x + PADDING_MM, cell.max.y - PADDING_MM - cap_mm)
}

/// The box a cell's value goes in, under a label `label_cap_mm` high when it has one (`cell`: the
/// cell inside its lines).
pub fn value_box(cell: Aabb2, label_cap_mm: Option<f64>) -> Aabb2 {
    let inset = cell.inflated(-PADDING_MM);
    match label_cap_mm {
        Some(cap_mm) => Aabb2 { max: Vec2::new(inset.max.x, label_at(cell, cap_mm).y - PADDING_MM), ..inset },
        None => inset,
    }
}

/// Where `lines` lines `cap_mm` high go in `value_box`, `pitch_mm` apart, by the module's rule:
/// the first line's anchor, each next line `pitch_mm` below the one before, and
/// [`Warning::Height`] when the last baseline is below the box or its descenders below the padding
/// under it. The box lies within the paper's coordinates ([`MAX_COORDINATE_MM`]), so the anchor is
/// always finite.
pub fn place_lines(value_box: Aabb2, lines: usize, cap_mm: f64, pitch_mm: f64, align: HAlign, wrap: bool) -> Result<(Vec2, Option<Warning>)> {
    let Aabb2 { min, max } = value_box;
    let on_paper = [min.x, min.y, max.x, max.y].into_iter().all(|v| v.abs() <= MAX_COORDINATE_MM);
    require(on_paper && min.x <= max.x && min.y <= max.y, "a value box is not inside out and lies within the paper's coordinates")?;
    require([cap_mm, pitch_mm].into_iter().all(|v| v.is_finite() && v > 0.0), "a cap height and a line pitch are finite and above 0")?;
    let x = match align {
        HAlign::Left => min.x,
        HAlign::Centre => value_box.center().x,
        HAlign::Right => max.x,
    };
    let y = if lines == 1 && !wrap { value_box.center().y - cap_mm / 2.0 } else { max.y - cap_mm };
    let last = y - lines.saturating_sub(1) as f64 * pitch_mm;
    // How far above the box's bottom the last baseline must be for its descenders to stay in the padding.
    let reach = (cap_mm * DESCENDER_OF_CAP - PADDING_MM).max(0.0);
    let overflows = lines > 0 && last - reach < min.y - EPS;
    Ok((Vec2::new(x, y), overflows.then_some(Warning::Height)))
}

#[cfg(test)]
#[path = "cell_tests.rs"]
mod tests;
