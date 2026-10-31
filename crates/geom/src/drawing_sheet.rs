//! Drawing sheets: paper sizes, orientation and the drawing border, as data in sheet millimetres.
//!
//! A [`Sheet`] is a paper size and an orientation; [`Sheet::size_mm`] gives its size as it lies, the
//! sheet border running round it. A [`DrawingBorderProfile`] is the rule for the drawing border
//! inside the sheet border: the margins between the two, the drawing border's line width, the grid
//! of reference fields with their labels, and the centring and trimming marks.
//! [`DrawingBorderProfile::validate`] checks a profile on its own, as when one is read, and
//! [`DrawingBorderProfile::layout`] works it out for one sheet, so a new size regrows it. Nothing
//! here is frozen geometry, saved in a design or drawn: a writer draws the [`SheetLayout`] later.
//!
//! Sheet coordinates are millimetres from the sheet's lower-left corner, x to the right and y up.
//! The drawing border's line is centred on [`SheetLayout::drawing_border`].
//!
//! [`DrawingBorderProfile::iso`] is the built-in profile, after ISO 5457: 10 mm margins all round,
//! a 0.7 mm drawing border, and 50 mm fields counted out from the sheet's centre lines, the corner
//! fields taking what is left. Rows are lettered from the top without I and O and columns numbered
//! from the left, on the sides [`LabelSides::Iso`] gives: the top and right of an A4 sheet, every
//! side of the others. The labels sit between the drawing border and the grid reference border,
//! 5 mm outside it. A centring mark runs across the whole margin, from the trimmed sheet's edge to
//! the drawing border, at each end of the sheet's centre lines, never into the drawing area.
//!
//! The rest of ISO 5457's border is a choice in the profile: the 20 mm filing margin on the left,
//! for drawings that get bound; centring marks that start at the grid reference border and reach
//! inside the drawing border (10 mm in the standard's figure); and a trimming mark at each corner, outside the grid reference border, of
//! two overlapping 10 x 5 mm rectangles. The built-in profile leaves the trimming marks out, as
//! they take a lot of ink.
//!
//! Sizes: ISO 216 A0 to A5, Letter and Legal, ANSI A to F (ASME Y14.1), the architectural A to E
//! and E1, and custom sizes, finite, above 0 and at most [`MAX_SIDE_MM`] a side. Title blocks sit on
//! the drawing border ([`crate::title_block`]). The elongated ISO sizes and the extra centring marks a
//! sheet larger than A0 needs (one in the middle of each section to be filmed) are not here yet.

use serde::{Deserialize, Serialize};

use crate::paper::MAX_LINE_MM;
/// The longest side a custom sheet may have, in millimetres: the paper scene's limit.
pub use crate::paper::MAX_SIDE_MM;
use crate::{Aabb2, Vec2};

/// The most fields an equal division may cut each way.
pub const MAX_EQUAL_FIELDS: u32 = 200;
/// ISO 5457's filing margin: the left margin of a drawing that gets bound.
pub const FILING_MARGIN_MM: f64 = 20.0;
const MIN_FIELD_MM: f64 = 1.0;
const MAX_FIELD_MM: f64 = 1000.0;
/// The smallest drawing space the margins may leave, each way.
const MIN_DRAWING_SPACE_MM: f64 = 1.0;
/// A corner field shorter than this joins its neighbour, so rounding never leaves a sliver.
const SLIVER_MM: f64 = 1e-9;
const INCH: f64 = 25.4;

/// A standard paper size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaperSize {
    A0,
    A1,
    A2,
    A3,
    A4,
    A5,
    Letter,
    Legal,
    AnsiA,
    AnsiB,
    AnsiC,
    AnsiD,
    AnsiE,
    AnsiF,
    ArchA,
    ArchB,
    ArchC,
    ArchD,
    ArchE,
    ArchE1,
}

impl PaperSize {
    pub const ALL: [PaperSize; 20] = {
        use PaperSize::*;
        [A0, A1, A2, A3, A4, A5, Letter, Legal, AnsiA, AnsiB, AnsiC, AnsiD, AnsiE, AnsiF, ArchA, ArchB, ArchC, ArchD, ArchE, ArchE1]
    };

    /// The short and the long side, in millimetres (the US sizes from their inches).
    pub fn sides_mm(self) -> (f64, f64) {
        use PaperSize::*;
        let inches = |short: f64, long: f64| (short * INCH, long * INCH);
        match self {
            A0 => (841.0, 1189.0),
            A1 => (594.0, 841.0),
            A2 => (420.0, 594.0),
            A3 => (297.0, 420.0),
            A4 => (210.0, 297.0),
            A5 => (148.0, 210.0),
            Letter | AnsiA => inches(8.5, 11.0),
            Legal => inches(8.5, 14.0),
            AnsiB => inches(11.0, 17.0),
            AnsiC => inches(17.0, 22.0),
            AnsiD => inches(22.0, 34.0),
            AnsiE => inches(34.0, 44.0),
            AnsiF => inches(28.0, 40.0),
            ArchA => inches(9.0, 12.0),
            ArchB => inches(12.0, 18.0),
            ArchC => inches(18.0, 24.0),
            ArchD => inches(24.0, 36.0),
            ArchE => inches(36.0, 48.0),
            ArchE1 => inches(30.0, 42.0),
        }
    }
}

/// A sheet's size: a standard one or any other.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SheetSize {
    Standard(PaperSize),
    /// The two sides in either order; the orientation decides which lies across.
    Custom {
        sides_mm: [f64; 2],
    },
}

/// Which way the sheet lies. Landscape puts the long side across.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    #[default]
    Landscape,
    Portrait,
}

/// One sheet: its size and the way it lies (landscape when not given).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sheet {
    pub size: SheetSize,
    #[serde(default)]
    pub orientation: Orientation,
}

/// The distances from the sheet border to the drawing border, in millimetres.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Margins {
    pub left_mm: f64,
    pub right_mm: f64,
    pub top_mm: f64,
    pub bottom_mm: f64,
}

impl Margins {
    /// Left, right, top and bottom.
    fn sides(self) -> [f64; 4] {
        [self.left_mm, self.right_mm, self.top_mm, self.bottom_mm]
    }
}

/// How the drawing border is cut into reference fields. Externally tagged (`"none"`,
/// `{"from_centre": {...}}`), as serde ignores extra fields on an internally tagged unit variant.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum GridRule {
    /// No reference fields.
    None,
    /// Fields of `field_mm` counted out from the sheet's centre lines, the corner fields taking
    /// what is left (ISO 5457, with 50 mm). Each way the larger half sets the count `n` (its
    /// length over `field_mm`, rounded, at least 1), and each half has `n` fields, or fewer where
    /// it is shorter.
    FromCentre { field_mm: f64 },
    /// The drawing border cut into equal fields.
    Equal { columns: u32, rows: u32 },
}

/// Sides of the drawing border.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sides {
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
}

impl Sides {
    pub const ALL: Sides = Sides { top: true, bottom: true, left: true, right: true };
}

/// Which sides of the drawing border carry the field labels: a rule for any sheet, so a profile
/// made for one size labels another correctly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum LabelSides {
    /// ISO 5457's: the top and right of an A4 sheet (either way), every side of the others.
    Iso,
    /// The same sides on every sheet.
    Fixed(Sides),
}

impl LabelSides {
    /// The sides labelled on `sheet`.
    pub fn on(self, sheet: &Sheet) -> Sides {
        match self {
            LabelSides::Iso if matches!(sheet.size, SheetSize::Standard(PaperSize::A4)) => {
                Sides { top: true, bottom: false, left: false, right: true }
            }
            LabelSides::Iso => Sides::ALL,
            LabelSides::Fixed(sides) => sides,
        }
    }
}

/// The rule for a drawing border and the marks around it: worked out for each sheet by
/// [`DrawingBorderProfile::layout`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrawingBorderProfile {
    pub margins: Margins,
    /// ISO 5457's filing margin: when on, the left margin is [`FILING_MARGIN_MM`] in place of
    /// `margins.left_mm`, which is kept for when it's off again. Off when not given.
    #[serde(default)]
    pub filing_margin: bool,
    /// The drawing border's line width; the centring marks are drawn in it too.
    pub drawing_border_line_mm: f64,
    pub grid: GridRule,
    /// Row letters skip I and O (they read as 1 and 0).
    pub skip_i_and_o: bool,
    pub label_sides: LabelSides,
    /// How far outside the drawing border the grid reference border runs; the labels sit between
    /// the two.
    pub grid_border_mm: f64,
    /// How far each centring mark reaches inside the drawing border, 0 to stop it on the drawing
    /// border, at most [`MAX_SIDE_MM`] (`None`: no centring marks).
    pub centring_mark_mm: Option<f64>,
    /// Where each centring mark starts outside the drawing border. The trimmed sheet's edge when not
    /// given.
    #[serde(default)]
    pub centring_mark_start: CentringMarkStart,
    /// The trimming mark's rectangles: their length along the sheet border, at most
    /// [`MAX_SIDE_MM`], and their width into the sheet, which must leave them outside the grid
    /// reference border (`None`: no trimming marks).
    pub trimming_mark_mm: Option<[f64; 2]>,
}

/// Where a centring mark starts, outside the drawing border.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CentringMarkStart {
    /// The trimmed sheet's edge, so the mark runs across the whole margin (the built-in sheet's).
    #[default]
    SheetEdge,
    /// The grid reference border, as ISO 5457's figure 5 draws it.
    GridBorder,
}

/// One reference field along its axis: `from_mm < to_mm` in sheet coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub from_mm: f64,
    pub to_mm: f64,
    pub label: String,
}

/// A sheet's drawing border and the marks around it, worked out: what a writer draws.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetLayout {
    pub sheet_border: Aabb2,
    pub drawing_border: Aabb2,
    pub drawing_border_line_mm: f64,
    /// Left to right, numbered from 1.
    pub columns: Vec<Field>,
    /// Top to bottom, lettered from A.
    pub rows: Vec<Field>,
    /// The sides labelled on this sheet.
    pub label_sides: Sides,
    pub grid_border: Aabb2,
    /// Bottom, top, left and right: each from the grid reference border to the drawing border, or
    /// as far inside it as the profile asks.
    pub centring_marks: Vec<[Vec2; 2]>,
    /// Filled rectangles, two at each corner of the sheet.
    pub trimming_marks: Vec<Aabb2>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("drawing sheet: {0}")]
pub struct Invalid(pub &'static str);

type Result<T> = std::result::Result<T, Invalid>;

fn require(condition: bool, message: &'static str) -> Result<()> {
    if condition { Ok(()) } else { Err(Invalid(message)) }
}

impl Sheet {
    pub fn standard(size: PaperSize, orientation: Orientation) -> Sheet {
        Sheet { size: SheetSize::Standard(size), orientation }
    }

    /// The sheet's width and height as it lies.
    pub fn size_mm(&self) -> Result<Vec2> {
        let (short, long) = match self.size {
            SheetSize::Standard(size) => size.sides_mm(),
            SheetSize::Custom { sides_mm: [a, b] } => {
                for side in [a, b] {
                    require(side.is_finite() && side > 0.0 && side <= MAX_SIDE_MM, "a custom side is finite, above 0 and at most 5000 mm")?;
                }
                (a.min(b), a.max(b))
            }
        };
        Ok(match self.orientation {
            Orientation::Landscape => Vec2::new(long, short),
            Orientation::Portrait => Vec2::new(short, long),
        })
    }
}

impl DrawingBorderProfile {
    /// The built-in drawing border, after ISO 5457, for a sheet of any size (the module's notes
    /// say how it follows the standard).
    pub fn iso() -> DrawingBorderProfile {
        DrawingBorderProfile {
            margins: Margins { left_mm: 10.0, right_mm: 10.0, top_mm: 10.0, bottom_mm: 10.0 },
            filing_margin: false,
            drawing_border_line_mm: 0.7,
            grid: GridRule::FromCentre { field_mm: 50.0 },
            skip_i_and_o: true,
            label_sides: LabelSides::Iso,
            grid_border_mm: 5.0,
            centring_mark_mm: Some(0.0),
            centring_mark_start: CentringMarkStart::SheetEdge,
            trimming_mark_mm: None,
        }
    }

    /// The margins as laid out: `margins`, with the filing margin on the left when it's on.
    fn laid_out_margins(&self) -> Margins {
        let left_mm = if self.filing_margin { FILING_MARGIN_MM } else { self.margins.left_mm };
        Margins { left_mm, ..self.margins }
    }

    /// Checks what doesn't depend on the sheet: call it when a profile is read. [`Self::layout`]
    /// calls it too.
    pub fn validate(&self) -> Result<()> {
        // Every margin as given, the left one too while the filing margin stands in for it, so
        // turning the filing margin off never brings back a bad value.
        for margin in self.margins.sides() {
            require(margin.is_finite() && (0.0..=MAX_SIDE_MM).contains(&margin), "a margin is finite and 0 to 5000 mm")?;
        }
        let line = self.drawing_border_line_mm;
        require(line.is_finite() && line > 0.0 && line <= MAX_LINE_MM, "the drawing border's line is above 0 and at most 10 mm wide")?;
        match self.grid {
            GridRule::None => {}
            GridRule::FromCentre { field_mm } => {
                require(field_mm.is_finite() && (MIN_FIELD_MM..=MAX_FIELD_MM).contains(&field_mm), "a field is 1 to 1000 mm long")?
            }
            GridRule::Equal { columns, rows } => {
                let counts = 1..=MAX_EQUAL_FIELDS;
                require(counts.contains(&columns) && counts.contains(&rows), "an equal division has 1 to 200 fields each way")?
            }
        }
        // The grid reference border lies in the margins, and the trimming marks between it and the
        // sheet border (ISO 5457 4.5), so the narrowest margin laid out bounds both.
        let narrowest = self.laid_out_margins().sides().into_iter().fold(f64::INFINITY, f64::min);
        let g = self.grid_border_mm;
        require(g.is_finite() && g >= 0.0 && g <= narrowest, "the grid reference border lies between the sheet border and the drawing border")?;
        if let Some(reach) = self.centring_mark_mm {
            let fits = reach.is_finite() && (0.0..=MAX_SIDE_MM).contains(&reach);
            require(fits, "a centring mark ends on the drawing border or at most 5000 mm inside it")?;
            // Else it would start and end on the drawing border.
            let outside = match self.centring_mark_start {
                CentringMarkStart::SheetEdge => narrowest,
                CentringMarkStart::GridBorder => g,
            };
            require(outside > 0.0 || reach > 0.0, "a centring mark needs room outside the drawing border to start from, or a reach inside it")?;
        }
        if let Some([length, width]) = self.trimming_mark_mm {
            let fits = length.is_finite() && length <= MAX_SIDE_MM && width > 0.0 && width <= length;
            require(fits, "a trimming mark is above 0, at most 5000 mm long and no wider than it is long")?;
            require(width <= narrowest - g, "a trimming mark lies between the sheet border and the grid reference border")?;
        }
        Ok(())
    }

    /// The drawing border and the marks around it on `sheet`.
    pub fn layout(&self, sheet: &Sheet) -> Result<SheetLayout> {
        self.validate()?;
        let size = sheet.size_mm()?;
        let m = self.laid_out_margins();
        let drawing_border = Aabb2 { min: Vec2::new(m.left_mm, m.bottom_mm), max: Vec2::new(size.x - m.right_mm, size.y - m.top_mm) };
        let space = drawing_border.size();
        require(space.x >= MIN_DRAWING_SPACE_MM && space.y >= MIN_DRAWING_SPACE_MM, "the margins leave no drawing space")?;
        let (across, down) = match self.grid {
            GridRule::None => (vec![], vec![]),
            GridRule::FromCentre { field_mm } => (
                from_centre(drawing_border.min.x, drawing_border.max.x, size.x / 2.0, field_mm)?,
                from_centre(drawing_border.min.y, drawing_border.max.y, size.y / 2.0, field_mm)?,
            ),
            GridRule::Equal { columns, rows } => {
                (equal(drawing_border.min.x, drawing_border.max.x, columns), equal(drawing_border.min.y, drawing_border.max.y, rows))
            }
        };
        let columns = fields(&across, |i| (i + 1).to_string());
        // Rows run top to bottom: the edges are in rising y, so the top row comes last.
        let mut rows = fields(&down, |_| String::new());
        rows.reverse();
        for (i, row) in rows.iter_mut().enumerate() {
            row.label = letters(i, self.skip_i_and_o);
        }
        let sheet_border = Aabb2 { min: Vec2::ZERO, max: size };
        let grid_border = drawing_border.inflated(self.grid_border_mm);
        let start = match self.centring_mark_start {
            CentringMarkStart::SheetEdge => sheet_border,
            CentringMarkStart::GridBorder => grid_border,
        };
        let centring_marks = match self.centring_mark_mm {
            Some(reach) => centring_marks(start, drawing_border, size, reach)?,
            None => Vec::new(),
        };
        let trimming_marks = match self.trimming_mark_mm {
            Some(mark) => trimming_marks(size, mark)?,
            None => Vec::new(),
        };
        Ok(SheetLayout {
            sheet_border,
            drawing_border,
            drawing_border_line_mm: self.drawing_border_line_mm,
            columns,
            rows,
            label_sides: self.label_sides.on(sheet),
            grid_border,
            centring_marks,
            trimming_marks,
        })
    }
}

/// The four centring marks: on the sheet's centre lines, from the edges of `start` to `reach`
/// inside the drawing border, which must not take them past the centre.
fn centring_marks(start: Aabb2, drawing_border: Aabb2, size: Vec2, reach: f64) -> Result<Vec<[Vec2; 2]>> {
    let (cx, cy) = (size.x / 2.0, size.y / 2.0);
    let (lo, hi) = (drawing_border.min, drawing_border.max);
    let room = (cx - lo.x).min(hi.x - cx).min(cy - lo.y).min(hi.y - cy);
    require(reach <= room, "a centring mark stops at the sheet's centre lines")?;
    Ok(vec![
        [Vec2::new(cx, start.min.y), Vec2::new(cx, lo.y + reach)],
        [Vec2::new(cx, start.max.y), Vec2::new(cx, hi.y - reach)],
        [Vec2::new(start.min.x, cy), Vec2::new(lo.x + reach, cy)],
        [Vec2::new(start.max.x, cy), Vec2::new(hi.x - reach, cy)],
    ])
}

/// The trimming marks: at each corner of the sheet, a rectangle `length` along each edge and
/// `width` into the sheet, the two overlapping in the corner.
fn trimming_marks(size: Vec2, [length, width]: [f64; 2]) -> Result<Vec<Aabb2>> {
    // Any longer and the two marks on one edge would meet.
    require(length <= size.x.min(size.y) / 2.0, "a trimming mark is at most half the sheet's shorter side long")?;
    // The span `extent` long from one end of `0..side`, the far end when `far`.
    let span = |far: bool, side: f64, extent: f64| if far { (side - extent, side) } else { (0.0, extent) };
    let mut marks = Vec::with_capacity(8);
    for (right, top) in [(false, false), (true, false), (false, true), (true, true)] {
        for (across, up) in [(length, width), (width, length)] {
            let ((x0, x1), (y0, y1)) = (span(right, size.x, across), span(top, size.y, up));
            marks.push(Aabb2 { min: Vec2::new(x0, y0), max: Vec2::new(x1, y1) });
        }
    }
    Ok(marks)
}

/// The field edges from `lo` to `hi`, in rising order, for fields of `field` out from `centre`.
fn from_centre(lo: f64, hi: f64, centre: f64, field: f64) -> Result<Vec<f64>> {
    let (below, above) = (centre - lo, hi - centre);
    require(below > SLIVER_MM && above > SLIVER_MM, "the sheet's centre lines cross the drawing border")?;
    // At most MAX_SIDE_MM / MIN_FIELD_MM per half: a bounded count.
    let n = (below.max(above) / field).round().max(1.0);
    let count = |half: f64| n.min(((half - SLIVER_MM) / field).ceil()) as usize;
    let (k_below, k_above) = (count(below), count(above));
    let mut edges = Vec::with_capacity(k_below + k_above + 1);
    edges.push(lo);
    edges.extend((1..k_below).rev().map(|j| centre - j as f64 * field));
    edges.push(centre);
    edges.extend((1..k_above).map(|j| centre + j as f64 * field));
    edges.push(hi);
    Ok(edges)
}

/// The edges of `n` equal fields from `lo` to `hi`, ending exactly at `hi`.
fn equal(lo: f64, hi: f64, n: u32) -> Vec<f64> {
    let step = (hi - lo) / f64::from(n);
    (0..n).map(|i| lo + f64::from(i) * step).chain(std::iter::once(hi)).collect()
}

fn fields(edges: &[f64], label: impl Fn(usize) -> String) -> Vec<Field> {
    edges
        .windows(2)
        .enumerate()
        .filter_map(|(i, w)| match *w {
            [from_mm, to_mm] => Some(Field { from_mm, to_mm, label: label(i) }),
            _ => None,
        })
        .collect()
}

/// The row label for row `i` (0 is A): A to Z, then AA, AB and on (without I and O if asked).
pub fn letters(i: usize, skip_i_and_o: bool) -> String {
    const ALL: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    const NO_I_O: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
    let alphabet = if skip_i_and_o { NO_I_O } else { ALL };
    let base = alphabet.len();
    let mut out = Vec::new();
    // Bijective numbering: after the last letter comes AA, not BA.
    let mut n = i.saturating_add(1);
    while n > 0 {
        n -= 1;
        out.extend(alphabet.get(n % base).map(|&b| char::from(b)));
        n /= base;
    }
    out.iter().rev().collect()
}

#[cfg(test)]
#[path = "drawing_sheet_tests.rs"]
mod tests;
