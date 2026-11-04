//! A drawing's saved tables (#3874): a parts list or a revision table on one of its sheets. The
//! record says what the table lists, how it reads ([`Table`]) and where it sits; its rows are never
//! saved, so it reads the design and the revision record as they are when it is drawn.
//!
//! - **A parts list** lists one bill of materials view of a component of the drawing's design,
//!   with the options [`crate::bom::ItemNumbers`] saves. Its source is checked against the
//!   drawing's sources; a component deleted since is for what draws it to report.
//! - **Where it sits** is a corner of the sheet's drawing space with an offset, so a change of
//!   paper takes it along and moves nothing else, or a free place on paper.
//!
//! The rules, beside the record's: a table is on one of the drawing's sheets; its definition keeps
//! [`Table::validate`]; its place is on paper ([`is_paper_coordinate`]); at most [`MAX_TABLES`],
//! listed by id.

use serde::{Deserialize, Serialize};
use solidcraft_geom::cell::{Content, Format, HAlign, Part, Source};
use solidcraft_geom::drawing_view::is_paper_coordinate;
use solidcraft_geom::paper::MAX_SIDE_MM;
use solidcraft_geom::table::{Column, Filter, Heading, Table};

use super::by_id::Record;
use super::{Drawing, SheetComponent, require};
use crate::bom::{Group, Request, View};
use crate::{DocError, Result, ids};

/// The most tables a drawing may have.
pub const MAX_TABLES: usize = 1_000;
/// The most decimals a mass may show.
const MAX_MASS_DECIMALS: u8 = 6;

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
}

impl Kind {
    /// The definition a new table of this kind starts with: headings on top, rows of 7 mm, each
    /// column as wide as its text.
    pub fn default_table(&self) -> Table {
        let columns = match self {
            Kind::PartsList(_) => vec![
                column("item", "ITEM", HAlign::Centre),
                column("quantity", "QTY", HAlign::Centre),
                column("name", "NAME", HAlign::Left),
                column("description", "DESCRIPTION", HAlign::Left),
                column("material", "MATERIAL", HAlign::Left),
                column("mass", "MASS", HAlign::Right),
            ],
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
        }
    }

    /// Where a new table of this kind goes: a parts list at the top right, a revision table at the
    /// bottom left.
    pub fn default_place(&self) -> Place {
        let corner = match self {
            Kind::PartsList(_) => Corner::TopRight,
            Kind::Revision => Corner::BottomLeft,
        };
        Place::Anchor { corner, offset_mm: [0.0, 0.0] }
    }
}

fn column(key: &str, heading: &str, align: HAlign) -> Column {
    let content = Content(vec![Part::Field { source: Source::Row, key: key.into(), format: Format::default() }]);
    Column { name: key.into(), heading: heading.into(), content, width_mm: None, align }
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
        if let Kind::PartsList(list) = &t.kind {
            let named = || format!("it lists source {}, which the drawing doesn't have", ids::encode(list.of.source));
            require(self.sources.iter().any(|s| s.id == list.of.source), named)?;
            require(list.mass.decimals <= MAX_MASS_DECIMALS, || format!("a mass shows at most {MAX_MASS_DECIMALS} decimals"))?;
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
