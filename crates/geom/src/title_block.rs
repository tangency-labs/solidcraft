//! Title blocks: the block of cells that identifies a drawing and what its sheet documents, as
//! data in sheet millimetres.
//!
//! A [`TitleBlock`] is a definition: its size, its two line widths and its cells. Each [`Cell`] is
//! a rectangle measured from the block's lower-right corner (x at most 0, y at least 0), with a
//! label and a value: text from cell content ([`crate::cell`]), or the projection symbol. The block
//! sits on the drawing border's lower-right corner, so one definition serves every sheet it fits.
//! [`default_block`] is the built-in definition. [`TitleBlock::validate`] checks a definition on
//! its own, as when one is read, and
//! [`TitleBlock::layout`] places it on one sheet by the cell text rule: each cell's rectangle, where
//! its label goes and the box its value goes in, and [`TitleBlockLayout::paper`] draws its lines and
//! the projection symbol ([`projection_symbol`]) in the drawing's projection. Text values are set in
//! their boxes once the paper scene has text.
//!
//! The outline is drawn `outline_line_mm` wide and the lines between cells `cell_line_mm`, each
//! centred on its edge. The block's right and bottom edges lie on the drawing border, whose line is
//! centred there too. A cell's text is padded from the inner edge of its lines and of the border's
//! line, wherever that reaches, and its descenders never reach a line (the cell text rule), so no
//! line covers it.

use serde::{Deserialize, Serialize};

use crate::cell::{self, Content, Fit, Format, HAlign, Part, Projection, Source, is_snake_case};
use crate::drawing_sheet::SheetLayout;
use crate::paper::{Group, Item, MAX_LINE_MM, ObjectKind};
use crate::{Aabb2, Loop2, Seg2, Vec2};

/// The most cells a block may have.
pub const MAX_CELLS: usize = 64;
const MIN_BLOCK_SIDE_MM: f64 = 1.0;
const MAX_BLOCK_SIDE_MM: f64 = 1000.0;

/// The projection symbol's sizes (ISO 5456-2, table A.1): `h`, the diameter of the cone's smaller
/// end, which is also the height of the capital letters it goes with.
pub const SYMBOL_SIZES_MM: [f64; 6] = [3.5, 5.0, 7.0, 10.0, 14.0, 20.0];

/// A title block's definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TitleBlock {
    /// The block's width and height.
    pub size_mm: Vec2,
    pub outline_line_mm: f64,
    pub cell_line_mm: f64,
    pub cells: Vec<Cell>,
}

/// One cell of a block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    /// Snake case, and no other cell of the block has it.
    pub name: String,
    /// `[x0, y0, x1, y1]` from the block's lower-right corner: `-width <= x0 < x1 <= 0` and
    /// `0 <= y0 < y1 <= height`.
    pub rect_mm: [f64; 4],
    /// None: the value has the whole cell.
    pub label: Option<Label>,
    pub kind: CellKind,
}

/// A cell's label, such as "Created by": one line at its top left.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Label {
    pub text: String,
    pub cap_mm: f64,
}

/// What a cell's value is. Externally tagged (`{"text": {...}}`, `"projection_symbol"`), as serde
/// ignores extra fields on an internally tagged unit variant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CellKind {
    Text(TextCell),
    /// The projection symbol, drawn from the drawing's projection (`drawing.projection`), which the
    /// views read too: never fixed in the block.
    ProjectionSymbol,
}

/// A text value: its content, its cap height, where its lines sit across the value box (left when
/// not given) and what it should fit (nothing when not given).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextCell {
    pub content: Content,
    pub cap_mm: f64,
    #[serde(default)]
    pub align: HAlign,
    #[serde(default)]
    pub fit: Fit,
    /// The drawing's image file shown in place of the text: a logo (ISO 7200: a name or a logo).
    #[serde(default, with = "crate::ids::option", skip_serializing_if = "Option::is_none")]
    pub image: Option<u64>,
}

/// A block placed on one sheet: what a writer draws, before the values are set.
#[derive(Clone, Debug, PartialEq)]
pub struct TitleBlockLayout {
    pub outline: Aabb2,
    pub outline_line_mm: f64,
    pub cell_line_mm: f64,
    /// One for each of the definition's cells, in its order.
    pub cells: Vec<CellLayout>,
}

/// One cell placed on a sheet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellLayout {
    pub rect: Aabb2,
    /// Where the label's baseline starts, when the cell has one.
    pub label_at: Option<Vec2>,
    pub value_box: Aabb2,
    /// The projection symbol's size `h`, when the cell holds it: the largest of [`SYMBOL_SIZES_MM`]
    /// whose symbol fits the value box, lines and all.
    pub symbol_mm: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("title block: {0}")]
pub struct Invalid(pub &'static str);

type Result<T> = std::result::Result<T, Invalid>;

fn require(condition: bool, message: &'static str) -> Result<()> {
    if condition { Ok(()) } else { Err(Invalid(message)) }
}

impl Cell {
    /// The cell's rectangle with the block's lower-right corner at `corner`.
    fn rect(&self, corner: Vec2) -> Aabb2 {
        let [x0, y0, x1, y1] = self.rect_mm;
        Aabb2 { min: corner + Vec2::new(x0, y0), max: corner + Vec2::new(x1, y1) }
    }

    /// The box its value goes in, in `inside`, the cell inside its lines.
    fn value_box(&self, inside: Aabb2) -> Aabb2 {
        cell::value_box(inside, self.label.as_ref().map(|label| label.cap_mm))
    }
}

/// The largest of [`SYMBOL_SIZES_MM`] whose symbol fits `room`: `4.3 h` by `2 h` on its lines'
/// centres, so `4.4 h` by `2.1 h` with its `h / 10` lines.
fn symbol_size(room: Vec2) -> Option<f64> {
    SYMBOL_SIZES_MM.into_iter().rev().find(|h| 4.4 * h <= room.x && 2.1 * h <= room.y)
}

/// The projection symbol in `projection`, `h` across the cone's smaller end and centred on `centre`
/// (ISO 5456-2, figures A.1 and A.2): a truncated cone `2 h` long, from its `h` end on the left to its
/// `2 h` end, and the cone seen end on, circles `2 h` and `h` across, `3 h / 10` from it: on the right
/// in first angle, on the left in third. Its lines are `h / 10` wide. The centrelines, which the
/// standard lets go, are left out.
pub fn projection_symbol(projection: Projection, h: f64, centre: Vec2) -> [Item; 3] {
    let gap = 0.3 * h;
    let left = centre.x - (4.0 * h + gap) / 2.0;
    let (cone_at, circles_at) = match projection {
        Projection::First => (left, left + 3.0 * h + gap),
        Projection::Third => (left + 2.0 * h + gap, left + h),
    };
    let y = centre.y;
    let cone = [(cone_at, y - h / 2.0), (cone_at + 2.0 * h, y - h), (cone_at + 2.0 * h, y + h), (cone_at, y + h / 2.0)];
    let cone = Loop2::polygon(&cone.map(|(x, y)| Vec2::new(x, y)));
    let circles = Vec2::new(circles_at, y);
    let stroke = |path: Loop2| Item::Stroke { path: path.segs, closed: true, width_mm: h / 10.0 };
    [stroke(cone), stroke(Loop2::circle(circles, h)), stroke(Loop2::circle(circles, h / 2.0))]
}

/// The built-in block, 180 x 49 mm: the twelve cells of the layout approved on #3875, each under a
/// 2.5 mm label, and no type, status or scale cell. Values come from the component the sheet
/// documents, the drawing, its newest revision and the sheet; the legal owner is the drawing's
/// `organization`, and the general tolerances are text to change.
pub fn default_block() -> TitleBlock {
    use HAlign::{Centre, Left};
    use Source::{Component, Drawing, Revision};
    let field = |source, key: &str| Part::Field { source, key: key.into(), format: Format::default() };
    let text = |cap_mm, align, fit, parts| CellKind::Text(TextCell { content: Content(parts), cap_mm, align, fit, image: None });
    let most = |n| Fit { max_chars: Some(n), ..Fit::default() };
    let (whole, wrap) = (Fit::default(), Fit { wrap: true, ..Fit::default() });
    let title = Fit { max_chars: Some(25), wide_max_chars: Some(30), wrap: false };
    let sheet = vec![field(Source::Sheet, "number"), Part::Text("/".into()), field(Source::Sheet, "count")];
    let tolerances = vec![Part::Text("ISO 2768-m".into())];
    let cells = [
        ("title", [-180.0, 0.0, -68.0, 14.0], "Title", text(5.0, Left, title, vec![field(Component, "name")])),
        ("identification_number", [-68.0, 0.0, 0.0, 14.0], "Identification number", text(5.0, Left, most(16), vec![field(Drawing, "number")])),
        ("created_by", [-180.0, 14.0, -121.0, 24.0], "Created by", text(3.5, Left, most(20), vec![field(Revision, "designer")])),
        ("approved_by", [-121.0, 14.0, -62.0, 24.0], "Approved by", text(3.5, Left, most(20), vec![field(Revision, "checker")])),
        ("date_of_issue", [-62.0, 14.0, -30.0, 24.0], "Date of issue", text(3.5, Left, most(10), vec![field(Revision, "date")])),
        ("sheet", [-30.0, 14.0, 0.0, 24.0], "Sheet", text(3.5, Centre, most(9), sheet)),
        ("material", [-180.0, 24.0, -100.0, 34.0], "Material", text(3.5, Left, whole, vec![field(Component, "material")])),
        ("finish", [-100.0, 24.0, -30.0, 34.0], "Finish", text(3.5, Left, whole, vec![field(Component, "properties.finish")])),
        ("mass", [-30.0, 24.0, 0.0, 34.0], "Mass", text(3.5, Centre, whole, vec![field(Component, "mass")])),
        ("legal_owner", [-180.0, 34.0, -100.0, 49.0], "Legal owner", text(3.5, Left, wrap, vec![field(Drawing, "organization")])),
        ("general_tolerances", [-100.0, 34.0, -30.0, 49.0], "General tolerances", text(2.5, Left, wrap, tolerances)),
        ("projection", [-30.0, 34.0, 0.0, 49.0], "Projection", CellKind::ProjectionSymbol),
    ];
    let label = |text: &str| Some(Label { text: text.into(), cap_mm: 2.5 });
    let cells = cells.into_iter().map(|(name, rect_mm, text, kind)| Cell { name: name.into(), rect_mm, label: label(text), kind }).collect();
    TitleBlock { size_mm: Vec2::new(180.0, 49.0), outline_line_mm: 0.7, cell_line_mm: 0.35, cells }
}

impl TitleBlock {
    /// The image files its cells show in place of their text.
    pub fn logos(&self) -> impl Iterator<Item = u64> + '_ {
        self.cells.iter().filter_map(|cell| match &cell.kind {
            CellKind::Text(text) => text.image,
            CellKind::ProjectionSymbol => None,
        })
    }

    /// `cell`'s rectangle `rect` inside its lines: each side moved in by half the line drawn on it,
    /// the outline's on the block's edges and a cell line's inside the block.
    fn inside_lines(&self, cell: &Cell, rect: Aabb2) -> Aabb2 {
        let [x0, y0, x1, y1] = cell.rect_mm;
        let half = |on_outline: bool| if on_outline { self.outline_line_mm } else { self.cell_line_mm } / 2.0;
        let lower = Vec2::new(half(x0 == -self.size_mm.x), half(y0 == 0.0));
        let upper = Vec2::new(half(x1 == 0.0), half(y1 == self.size_mm.y));
        Aabb2 { min: rect.min + lower, max: rect.max - upper }
    }

    /// Each cell with the block's lower-right corner at `corner`, inside its lines and inside
    /// `clear` too when given (the drawing border's line's inner edge), checked by the cell text
    /// rule: room for a value below its label, for one line of a text value, and for the smallest
    /// projection symbol.
    fn place_cells(&self, corner: Vec2, clear: Option<Aabb2>) -> Result<Vec<CellLayout>> {
        let place = |cell: &Cell| {
            let rect = cell.rect(corner);
            let inside = self.inside_lines(cell, rect);
            let inside = clear.map_or(inside, |clear| inside.intersection(&clear));
            let value_box = cell.value_box(inside);
            let room = value_box.size();
            require(room.x > 0.0 && room.y > 0.0, "a cell has room for its value below its label")?;
            let symbol_mm = match &cell.kind {
                CellKind::Text(text) => {
                    let one_line = cell::place_lines(value_box, 1, text.cap_mm, text.cap_mm, text.align, false);
                    require(matches!(one_line, Ok((_, None))), "a value's cap height is above 0 and one line of it fits its value box")?;
                    None
                }
                CellKind::ProjectionSymbol => {
                    let size = symbol_size(room);
                    require(size.is_some(), "a projection symbol's cell has room for its smallest size, 15.4 x 7.35 mm")?;
                    size
                }
            };
            let label_at = cell.label.as_ref().map(|label| cell::label_at(inside, label.cap_mm));
            Ok(CellLayout { rect, label_at, value_box, symbol_mm })
        };
        self.cells.iter().map(place).collect()
    }

    /// Checks a definition on its own, counting its cells first: call it when one is read. Its
    /// cells' room is checked with the corner at the origin and only the outline on its edges;
    /// [`Self::layout`] calls it too, and checks their room again where the block sits.
    pub fn validate(&self) -> Result<()> {
        require(self.cells.len() <= MAX_CELLS, "a title block has at most 64 cells")?;
        let Vec2 { x: width, y: height } = self.size_mm;
        let sides = MIN_BLOCK_SIDE_MM..=MAX_BLOCK_SIDE_MM;
        require(sides.contains(&width) && sides.contains(&height), "each side of a title block is 1 to 1000 mm")?;
        let lines = [self.outline_line_mm, self.cell_line_mm];
        require(lines.into_iter().all(|line| line > 0.0 && line <= MAX_LINE_MM), "a title block's lines are above 0 and at most 10 mm wide")?;
        for (i, cell) in self.cells.iter().enumerate() {
            let earlier = || self.cells.iter().take(i);
            require(is_snake_case(&cell.name), "a cell's name is snake case and at most 64 characters")?;
            require(earlier().all(|other| other.name != cell.name), "no two cells have the same name")?;
            let [x0, y0, x1, y1] = cell.rect_mm;
            require(-width <= x0 && x0 < x1 && x1 <= 0.0 && 0.0 <= y0 && y0 < y1 && y1 <= height, "a cell lies inside its block")?;
            let rect = cell.rect(Vec2::ZERO);
            require(earlier().all(|other| !other.rect(Vec2::ZERO).overlaps(&rect)), "no two cells overlap")?;
            if let Some(label) = &cell.label {
                require(label.cap_mm.is_finite() && label.cap_mm > 0.0, "a label's cap height is finite and above 0")?;
                cell::check_label(&label.text).map_err(|cell::Invalid(message)| Invalid(message))?;
            }
            if let CellKind::Text(text) = &cell.kind {
                text.content.validate().map_err(|cell::Invalid(message)| Invalid(message))?;
                let row = text.content.0.iter().any(|part| matches!(part, Part::Field { source: Source::Row, .. }));
                require(!row, "a title block's cells have no row to read")?;
            }
        }
        self.place_cells(Vec2::ZERO, None)?;
        Ok(())
    }

    /// The block on the lower-right corner of `sheet`'s drawing border, which must hold it, each
    /// cell with room inside the border's line too.
    pub fn layout(&self, sheet: &SheetLayout) -> Result<TitleBlockLayout> {
        self.validate()?;
        let border = sheet.drawing_border;
        let (size, room) = (self.size_mm, border.size());
        let fits = border.min.is_finite() && border.max.is_finite() && size.x <= room.x && size.y <= room.y;
        require(fits, "a title block fits inside the drawing border")?;
        let corner = Vec2::new(border.max.x, border.min.y);
        // Checked again here: the border's line, centred on it, runs along the block's right and
        // bottom edges (and any other it reaches), and rounding at the real corner may take the
        // last of a cell's room.
        let cells = self.place_cells(corner, Some(border.inflated(-sheet.drawing_border_line_mm / 2.0)))?;
        Ok(TitleBlockLayout {
            outline: Aabb2 { min: corner - Vec2::new(size.x, 0.0), max: corner + Vec2::new(0.0, size.y) },
            outline_line_mm: self.outline_line_mm,
            cell_line_mm: self.cell_line_mm,
            cells,
        })
    }
}

impl TitleBlockLayout {
    /// The block as one paper group of `object` (its sheet): the outline as a closed stroke
    /// `outline_line_mm` wide, then every cell edge inside it `cell_line_mm` wide, across from the top
    /// down and then up and down from the left, then the projection symbol in `projection` centred
    /// in each cell that holds it. Each line is drawn once, centred on its edge: neighbours' shared
    /// edges, and edges that meet end to end along one line, are one stroke. The cells' text isn't
    /// here yet.
    pub fn paper(&self, object: u64, projection: Projection) -> Group {
        let Aabb2 { min, max } = self.outline;
        let corners = [min, Vec2::new(max.x, min.y), max, Vec2::new(min.x, max.y)];
        let mut items = vec![Item::Stroke { path: Loop2::polygon(&corners).segs, closed: true, width_mm: self.outline_line_mm }];
        // Each cell edge off the outline, as (the line it is on, where it starts, where it ends).
        let (mut across, mut down) = (Vec::new(), Vec::new());
        for Aabb2 { min: a, max: b } in self.cells.iter().map(|cell| cell.rect) {
            across.extend([a.y, b.y].into_iter().filter(|y| *y != min.y && *y != max.y).map(|y| (y, a.x, b.x)));
            down.extend([a.x, b.x].into_iter().filter(|x| *x != min.x && *x != max.x).map(|x| (x, a.y, b.y)));
        }
        across.sort_by(|p, q| q.0.total_cmp(&p.0).then(p.1.total_cmp(&q.1)));
        down.sort_by(|p, q| p.0.total_cmp(&q.0).then(p.1.total_cmp(&q.1)));
        let line = |a, b| Item::Stroke { path: vec![Seg2::Line { a, b }], closed: false, width_mm: self.cell_line_mm };
        items.extend(joined(&across).into_iter().map(|(y, x0, x1)| line(Vec2::new(x0, y), Vec2::new(x1, y))));
        items.extend(joined(&down).into_iter().map(|(x, y0, y1)| line(Vec2::new(x, y0), Vec2::new(x, y1))));
        for cell in &self.cells {
            if let Some(h) = cell.symbol_mm {
                items.extend(projection_symbol(projection, h, cell.value_box.center()));
            }
        }
        Group { object, kind: ObjectKind::TitleBlock, items }
    }
}

/// `pieces`, sorted by the line they are on and then where they start, with the pieces on one line
/// that meet or overlap joined into one.
fn joined(pieces: &[(f64, f64, f64)]) -> Vec<(f64, f64, f64)> {
    let mut out: Vec<(f64, f64, f64)> = Vec::new();
    for &(on, from, to) in pieces {
        match out.last_mut() {
            Some(last) if last.0 == on && from <= last.2 => last.2 = last.2.max(to),
            _ => out.push((on, from, to)),
        }
    }
    out
}

#[cfg(test)]
#[path = "title_block_tests.rs"]
mod tests;
