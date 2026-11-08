//! A drawing's saved tables (#3874): a parts list or a revision table on one of its sheets. The
//! record says what the table lists, how it reads ([`Table`]) and where it sits. Only a general
//! table's rows are saved: every other kind reads the design and the revision record as they are
//! when it is drawn.
//!
//! - **A parts list** lists one bill of materials view of a component of the drawing's design,
//!   with the options [`crate::bom::ItemNumbers`] saves. Its source is checked against the
//!   drawing's sources; a component deleted since is for what draws it to report.
//!   [`PartsList::rows`] gives a list's rows as a table reads them.
//! - **A family table** lists the design's configurations, a row each, with a column per
//!   configuration column ([`Family::table`]). Only the design has configurations: one of another
//!   component is for what draws it to report.
//! - **A general table** holds cells typed in ([`General`]), as plain text. Its cells are the
//!   table's, so an override over one is refused: the cell itself is typed.
//! - **Where it sits** is a corner of the sheet's drawing space with an offset, so a change of
//!   paper takes it along and moves nothing else, or a free place on paper.
//!
//! The rules, beside the record's: a table is on one of the drawing's sheets; its definition keeps
//! [`Table::validate`]; its place is on paper ([`is_paper_coordinate`]); at most [`MAX_TABLES`],
//! listed by id. A general table's cells keep a table's caps (`General::check`).

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use solidcraft_geom::cell::{self, Content, Format, HAlign, MAX_NAME_BYTES, Part, Source};
use solidcraft_geom::drawing_view::is_paper_coordinate;
use solidcraft_geom::paper::MAX_SIDE_MM;
use solidcraft_geom::table::{self, BALLOONED, Column, Filter, Heading, Table};

use super::by_id::Record;
use super::{Drawing, SheetComponent, require};
use crate::assembly::BomType;
use crate::bom::{self, Bom, Group, Mass, Quantity, Request, View};
use crate::config::Column as ConfigColumn;
use crate::display_units::{NumberFormat, format_length, format_number};
use crate::{DocError, Document, Result, expr, ids};

/// The most tables a drawing may have.
pub const MAX_TABLES: usize = 1_000;
/// The most decimals a mass may show.
const MAX_MASS_DECIMALS: u8 = 6;
/// The row field that marks a family table's active configuration, which no new table prints.
pub const ACTIVE: &str = "active";

/// A table on a sheet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheetTable {
    #[serde(with = "crate::ids")]
    pub id: u64,
    /// The sheet it is on.
    #[serde(with = "crate::ids")]
    pub sheet: u64,
    pub kind: Kind,
    pub table: Table,
    pub place: Place,
}

/// What a table lists.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Kind {
    PartsList(PartsList),
    /// The drawing's revision rows ([`Drawing::revisions`]).
    Revision,
    Family(Family),
    General(General),
}

/// The cells a general table holds, typed in: each row its cells by column key, so a column moved
/// keeps its cells. An empty cell isn't saved, so a cell has one form; one no column reads is kept
/// and not drawn, as a parts list's field no column reads isn't.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct General {
    pub rows: Vec<BTreeMap<String, String>>,
}

/// How many configuration columns a new family table leaves out, by why.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LeftOut {
    /// Appearance columns: a look can't be printed as text.
    pub looks: usize,
    /// Those a table has no room for, that no row could fill or whose heading no label can show.
    pub others: usize,
}

/// The configurations a family table lists.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Family {
    /// The component whose configurations it lists (0: the design, the only one that has them).
    pub of: SheetComponent,
}

/// The bill of materials view a parts list lists, in the words [`crate::bom::ItemNumbers`] saves.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartsList {
    /// The component listed (0: the design).
    pub of: SheetComponent,
    #[serde(default, rename = "levels")]
    pub view: View,
    #[serde(default, rename = "group_by")]
    pub group: Group,
    #[serde(default, rename = "group_children", skip_serializing_if = "std::ops::Not::not")]
    pub fold: bool,
    #[serde(default, rename = "show_purchased_contents", skip_serializing_if = "std::ops::Not::not")]
    pub purchased_contents: bool,
    #[serde(default)]
    pub mass: MassFormat,
}

/// How a parts list shows a mass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MassFormat {
    pub unit: MassUnit,
    pub decimals: u8,
}

impl Default for MassFormat {
    fn default() -> Self {
        MassFormat { unit: MassUnit::Kg, decimals: 2 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MassUnit {
    G,
    Kg,
    Lb,
}

/// Where a table sits.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Place {
    /// The table's `corner` at that corner of the sheet's drawing space, moved by `offset_mm`.
    Anchor {
        corner: Corner,
        #[serde(default)]
        offset_mm: [f64; 2],
    },
    /// Its outline's top left, in millimetres from the sheet's lower-left corner.
    At([f64; 2]),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl PartsList {
    /// The list as the bill of materials asks for it, unweighed.
    pub fn request(&self) -> Request {
        Request {
            source: self.of.component,
            view: self.view,
            group: self.group,
            fold: self.fold,
            mass: false,
            purchased_contents: self.purchased_contents,
        }
    }

    /// Each row of `list` as a table reads it, each followed by the rows listed under it, with its
    /// values formatted and its numbers kept as numbers, so a sort orders them by value. A length is
    /// shown in `units`. A value the row doesn't have is left out, so the
    /// cell shows [`solidcraft_geom::cell::MISSING`]: a mass not weighed or not known, or a quantity that doesn't add up.
    pub fn rows(&self, list: &Bom, units: &str) -> Vec<table::Row> {
        let row = |(item, row): (Vec<u32>, &bom::Row)| {
            // Free properties first, so that none takes the place of a value below.
            let mut fields = row.properties.clone();
            let values = [
                ("item", bom::item_text(&item)),
                ("part_number", row.part_number.clone()),
                ("name", row.names.join("; ")),
                ("description", row.description.clone()),
                ("revision", row.revision.clone()),
                ("material", row.materials.join("; ")),
                ("count", row.count.to_string()),
            ];
            fields.extend(values.map(|(key, value)| (key.to_string(), value)));
            // In one of the row it is listed under, as a list reads; `count` has them all.
            let quantity = match row.quantity {
                Quantity::Count => Some(row.each.to_string()),
                Quantity::Pieces(n) => Some(n.to_string()),
                Quantity::Length(mm) => Some(format_length(mm, units, 2)),
                Quantity::Unknown(_) => None,
            };
            let values = [("quantity", quantity), ("mass", self.mass.text(row.mass))];
            fields.extend(values.into_iter().filter_map(|(key, value)| Some((key.to_string(), value?))));
            let quantity = match row.quantity {
                // A count's text is a whole number, which is exact as a float up to 2^53.
                Quantity::Count => Some(row.each as f64),
                Quantity::Pieces(n) => Some(n),
                Quantity::Length(mm) => Some(mm),
                Quantity::Unknown(_) => None,
            };
            // The values the texts above are formatted from, filled here with them (`table::Row::numbers`).
            let mass = if let Mass::Grams(grams) = row.mass { Some(grams) } else { None };
            let numbers = [("count", Some(row.count as f64)), ("quantity", quantity), ("mass", mass)];
            let numbers = numbers.into_iter().filter_map(|(key, n)| Some((key.to_string(), n.filter(|n| n.is_finite())?))).collect();
            let kind = if row.assembly { table::RowKind::Assembly } else { table::RowKind::Part };
            // The item's path names the row and those it is listed under: one deeper per member.
            let level = u32::try_from(item.len().saturating_sub(1)).unwrap_or(u32::MAX);
            table::Row { key: row.key.text(), kind, purchased: row.bom_type == BomType::Purchased, fields, level, numbers }
        };
        list.rows_deep().into_iter().map(row).collect()
    }
}

impl Family {
    /// The definition a new family table starts with: a new table's NAME column, then one for each
    /// of `doc`'s configuration columns in its order, as many as a table has room for, with those
    /// left out. An appearance column is left out: a look can't be printed as text. A column reads the cell its configuration column gives each row
    /// ([`Document::config_key`]); its heading is the parameter's name, or the column's kind and
    /// target, in capitals, as a parts list's headings are; its name is the key in snake case,
    /// numbered on a clash, so a filter, an override or a sort can name it.
    pub fn table(&self, doc: &Document) -> (Table, LeftOut) {
        let mut table = Kind::Family(*self).default_table();
        let mut taken: HashSet<String> = table.columns.iter().map(|c| c.name.clone()).collect();
        let mut left_out = LeftOut::default();
        for config in &doc.configs.columns {
            if matches!(config, ConfigColumn::Appearance(_) | ConfigColumn::ComponentAppearance(_) | ConfigColumn::FaceAppearance(_)) {
                left_out.looks += 1;
                continue;
            }
            if table.columns.len() == table::MAX_COLUMNS {
                left_out.others += 1;
                continue;
            }
            // A column no row could fill, or with a heading no label can show, is left out.
            let key = doc.config_key(config);
            let heading = heading_of(&key);
            if key.len() > table::MAX_KEY_BYTES || cell::check_label(&heading).is_err() {
                left_out.others += 1;
                continue;
            }
            let name = unique(snake_case(&key), &mut taken);
            table.columns.push(Column { name, ..column(&key, &heading, HAlign::Left) });
        }
        (table, left_out)
    }

    /// Each of `doc`'s configurations as a table reads it, in its table's order: its name, the cell
    /// of each configuration column, and [`ACTIVE`] `yes` on the active one. A parameter's cell
    /// carries the value it gives the configuration ([`Document::configuration_values`]) as its
    /// number, so it sorts by value; one worked out from other parameters prints that value, a
    /// length in `units`, and one typed as a value prints as typed. Any other cell is text.
    pub fn rows(doc: &Document, units: &str) -> Vec<table::Row> {
        let configs = &doc.configs;
        let keys: Vec<String> = configs.columns.iter().map(|c| doc.config_key(c)).collect();
        let reads_values = configs.columns.iter().any(|c| matches!(c, ConfigColumn::Param(_)));
        let row = |config: &crate::config::ConfigRow| {
            let values = if reads_values { doc.configuration_values(config) } else { BTreeMap::new() };
            let (mut fields, mut numbers) = (BTreeMap::new(), BTreeMap::new());
            for ((key, column), cell) in keys.iter().zip(&configs.columns).zip(&config.cells) {
                // A key longer than a row's field key can be names no column, so it is left out.
                if key.len() > table::MAX_KEY_BYTES {
                    continue;
                }
                let value = match column {
                    ConfigColumn::Param(name) => values.get(name).copied().filter(|v| v.v.is_finite() && v.kind().is_some()),
                    _ => None,
                };
                let text = match value {
                    Some(v) if !expr::references(cell).is_empty() => shown(v, units).unwrap_or_else(|| cell.clone()),
                    _ => cell.clone(),
                };
                if let Some(v) = value {
                    numbers.insert(key.clone(), v.v);
                }
                fields.insert(key.clone(), text);
            }
            fields.insert("name".into(), config.name.clone());
            if configs.active.as_deref() == Some(config.name.as_str()) {
                fields.insert(ACTIVE.into(), "yes".into());
            }
            table::Row { key: config.name.clone(), kind: table::RowKind::Part, purchased: false, fields, level: 0, numbers }
        };
        configs.rows.iter().map(row).collect()
    }
}

/// A parameter's value as a table prints it, with two decimals: a length in `units`, as a parts
/// list's lengths are, an angle in degrees, a number as it is.
fn shown(value: expr::Value, units: &str) -> Option<String> {
    let number = |x: f64| format_number(x, 2, &NumberFormat::default()).ok().map(|shown| shown.text);
    match value.kind()? {
        expr::Kind::Length => Some(format_length(value.v, units, 2)),
        expr::Kind::Angle => number(value.v.to_degrees()).map(|n| format!("{n}°")),
        expr::Kind::Unitless => number(value.v),
    }
}

/// A configuration column's heading: a parameter's name, or the column's kind and target, in
/// capitals. [`Document::config_key`] names a column `<kind>:<target>`.
fn heading_of(key: &str) -> String {
    let heading = match key.split_once(':') {
        Some(("param", name)) => name.to_string(),
        Some((kind, target)) => format!("{} {target}", kind.replace('_', " ")),
        None => key.to_string(),
    };
    heading.to_uppercase()
}

/// `key` as a column's name: its letters and digits in lower case, each run of anything else one
/// `_`, short enough for a `_N` to follow. A configuration key starts with its kind's letters.
fn snake_case(key: &str) -> String {
    let mut name = String::new();
    for c in key.chars() {
        if c.is_ascii_alphanumeric() {
            name.push(c.to_ascii_lowercase());
        } else if !name.is_empty() && !name.ends_with('_') {
            name.push('_');
        }
    }
    // All ASCII, so any length is a character boundary.
    name.truncate(MAX_NAME_BYTES - 8);
    name.trim_end_matches('_').to_string()
}

/// `base`, or `base_2`, `base_3`... the first that isn't `taken`, which it then is.
fn unique(base: String, taken: &mut HashSet<String>) -> String {
    let mut name = base.clone();
    let mut n = 2;
    while !taken.insert(name.clone()) {
        name = format!("{base}_{n}");
        n += 1;
    }
    name
}

impl General {
    /// Each row as `table` reads it: its key its number from 1, its fields its cells, and each cell
    /// typed as a number also as that number ([`table::typed_number`]), so `1.2` sorts before `1.10`.
    /// A cell left empty is blank, not missing: every field a column reads is there.
    pub fn rows(&self, table: &Table) -> Vec<table::Row> {
        let read = table.columns.iter().flat_map(|c| &c.content.0).filter_map(|part| match part {
            Part::Field { key, .. } => Some((key.clone(), String::new())),
            Part::Text(_) => None,
        });
        let blank: BTreeMap<String, String> = read.collect();
        let row = |(at, cells): (usize, &BTreeMap<String, String>)| {
            let numbers = cells.iter().filter_map(|(key, text)| Some((key.clone(), table::typed_number(text)?))).collect();
            let mut fields = blank.clone();
            fields.extend(cells.clone());
            let key = at.saturating_add(1).to_string();
            table::Row { key, kind: table::RowKind::Part, purchased: false, fields, level: 0, numbers }
        };
        self.rows.iter().enumerate().map(row).collect()
    }

    /// The rules: a table's caps on its rows, its columns and its text, each cell's text by the
    /// cell text rule and not empty, each key one line, and no overrides on `table`.
    fn check(&self, table: &Table) -> Result<()> {
        require(self.rows.len() <= table::MAX_ROWS, || format!("a general table has at most {} rows", table::MAX_ROWS))?;
        require(table.overrides.is_empty(), || "a general table's cells are typed in: type into the cell instead (`general.cells`)".into())?;
        let mut bytes = 0usize;
        for cells in &self.rows {
            require(cells.len() <= table::MAX_COLUMNS, || format!("a general table's row has at most {} cells", table::MAX_COLUMNS))?;
            for (key, text) in cells {
                let one_line = !key.is_empty() && key.len() <= table::MAX_KEY_BYTES && cell::check_label(key).is_ok();
                require(one_line, || format!("a cell's column key is one line of 1 to {} bytes", table::MAX_KEY_BYTES))?;
                require(!text.is_empty(), || format!("the cell `{key}` is empty: an empty cell isn't saved"))?;
                cell::check_text([text.as_str()]).map_err(|e| DocError::Invalid(e.to_string()))?;
                bytes = bytes.saturating_add(key.len()).saturating_add(text.len());
                require(bytes <= table::MAX_TEXT_BYTES, || "a general table's cells hold at most 1 MiB of text".into())?;
            }
        }
        Ok(())
    }
}

impl MassFormat {
    /// A mass as this format shows it, without its unit: the column's heading says that.
    fn text(self, mass: Mass) -> Option<String> {
        let Mass::Grams(grams) = mass else { return None };
        let in_grams = match self.unit {
            MassUnit::G => 1.0,
            MassUnit::Kg => 1000.0,
            MassUnit::Lb => 453.592_37,
        };
        format_number(grams / in_grams, self.decimals, &NumberFormat::default()).ok().map(|shown| shown.text)
    }
}

impl Kind {
    /// The definition a new table of this kind starts with: headings on top, rows of 7 mm, each
    /// column as wide as its text. A parts list's last column says how many balloons point at each
    /// row, for its editor alone: it isn't printed.
    pub fn default_table(&self) -> Table {
        let columns = match self {
            Kind::PartsList(_) => vec![
                column("item", "ITEM", HAlign::Centre),
                column("quantity", "QTY", HAlign::Centre),
                column("name", "NAME", HAlign::Left),
                column("description", "DESCRIPTION", HAlign::Left),
                column("material", "MATERIAL", HAlign::Left),
                column("mass", "MASS", HAlign::Right),
                Column { printed: false, ..column(BALLOONED, "BALLOONED", HAlign::Centre) },
            ],
            Kind::Family(_) => vec![column("name", "NAME", HAlign::Left)],
            Kind::General(_) => vec![column("a", "A", HAlign::Left), column("b", "B", HAlign::Left), column("c", "C", HAlign::Left)],
            Kind::Revision => vec![
                column("revision", "REV", HAlign::Centre),
                column("description", "DESCRIPTION", HAlign::Left),
                column("designer", "DRAWN", HAlign::Left),
                column("checker", "CHECKED", HAlign::Left),
                column("date", "DATE", HAlign::Left),
            ],
        };
        Table {
            columns,
            heading: Heading::Top,
            row_height_mm: 7.0,
            cap_mm: 3.5,
            outline_line_mm: 0.7,
            cell_line_mm: 0.35,
            filter: Filter::default(),
            overrides: Vec::new(),
            split: None,
            sort: Vec::new(),
        }
    }

    /// Where a new table of this kind goes: a parts list at the top right, a revision table at the
    /// bottom left, a family table and a general table at the top left.
    pub fn default_place(&self) -> Place {
        let corner = match self {
            Kind::PartsList(_) => Corner::TopRight,
            Kind::Revision => Corner::BottomLeft,
            Kind::Family(_) | Kind::General(_) => Corner::TopLeft,
        };
        Place::Anchor { corner, offset_mm: [0.0, 0.0] }
    }

    /// The component it lists, if it lists one.
    pub fn of(&self) -> Option<SheetComponent> {
        match self {
            Kind::PartsList(list) => Some(list.of),
            Kind::Family(family) => Some(family.of),
            Kind::Revision | Kind::General(_) => None,
        }
    }
}

fn column(key: &str, heading: &str, align: HAlign) -> Column {
    let content = Content(vec![Part::Field { source: Source::Row, key: key.into(), format: Format::default() }]);
    Column { name: key.into(), heading: heading.into(), content, width_mm: None, align, printed: true }
}

impl Record for SheetTable {
    fn id(&self) -> u64 {
        self.id
    }
}

impl Drawing {
    /// The tables' rules (the module's notes).
    pub(super) fn check_tables(&self) -> Result<()> {
        self.tables.check(MAX_TABLES, "table", |t| self.check_table(t))
    }

    fn check_table(&self, t: &SheetTable) -> Result<()> {
        require(self.sheets.iter().any(|s| s.id == t.sheet), || format!("it is on sheet {}, which the drawing doesn't have", ids::encode(t.sheet)))?;
        t.table.validate().map_err(|e| DocError::Invalid(e.to_string()))?;
        if let Some(of) = t.kind.of() {
            let named = || format!("it lists source {}, which the drawing doesn't have", ids::encode(of.source));
            require(self.sources.iter().any(|s| s.id == of.source), named)?;
        }
        match &t.kind {
            Kind::PartsList(list) => {
                require(list.mass.decimals <= MAX_MASS_DECIMALS, || format!("a mass shows at most {MAX_MASS_DECIMALS} decimals"))?;
            }
            Kind::General(general) => general.check(&t.table)?,
            Kind::Revision | Kind::Family(_) => {}
        }
        let [x, y] = match t.place {
            Place::Anchor { offset_mm, .. } => offset_mm,
            Place::At(at) => at,
        };
        require(is_paper_coordinate(x) && is_paper_coordinate(y), || format!("its place is at most {MAX_SIDE_MM} mm on paper"))
    }
}

#[cfg(test)]
#[path = "table_tests.rs"]
mod tests;
