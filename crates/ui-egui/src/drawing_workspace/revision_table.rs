//! A revision table's editor, in its own window ([`super::table_window`]) (#18983): the drawing's
//! revision rows in the kit's grid, each gesture run at once as one undo step. A committed cell, a
//! paste or a fill is one `drawing.revision.edit` for each row it touches, and a paste below the last
//! row adds rows with their texts (`drawing.revision.add`); [`SolveApp::run_as_one`] makes them one
//! step. The add-row control adds the next revision, and a row's Remove runs
//! `drawing.revision.delete`. A revision is issued by command (`drawing.revise`), not from here. What
//! the drawing refuses is said on the status line in its own words.
use std::borrow::Cow;

use serde_json::{Map, Value, json};
use solidcraft_engine::{
    doc::{drawing::table::Kind, ids},
    geom::revision::{Field, RevisionRecord, RevisionRow},
};

use crate::{
    SolveApp,
    ui_kit::{self, CellKind, Edit, GridColumn, GridSource, RowRef},
};

/// The grid's id: its state, and its cells' handles (`grid:drawing_revision_rows:{row}:{column}`).
const GRID: &str = "drawing_revision_rows";
/// Row controls at the edges, texts in the middle: Shown first, the five texts ([`Field::ALL`]),
/// then the row's Remove last. The window widens to show them all ([`super::table_window`]).
const SHOWN: usize = 0;
const TEXTS: usize = 1;
const ACTIONS: usize = TEXTS + Field::ALL.len();

/// The widest a text column other than Description grows to show its longest text, in points.
const TEXT_WIDEST: f32 = 220.0;

/// Description's narrowest and widest floor, in card widths (`form_card_width`): an empty table
/// still has room to type one, and a long one shows in full up to the widest, then elides.
/// Description is the one text column that takes the width the others leave.
const DESCRIPTION_CARDS: (f32, f32) = (1.0, 2.0);

/// The text field shown in grid column `col`, if it is one.
fn field(col: usize) -> Option<Field> {
    col.checked_sub(TEXTS).and_then(|i| Field::ALL.get(i)).copied()
}

fn tr(key: &'static str) -> &'static str {
    solidcraft_ui_text::tr(key)
}

/// The rows a revision table lists, for the grid: hidden ones too, since Shown is how a row is
/// hidden or shown again.
struct Rows<'a> {
    record: &'a RevisionRecord,
    rows: Vec<&'a RevisionRow>,
    /// Each text's heading as the table prints it.
    headings: [String; Field::ALL.len()],
    /// Each text column's width: its heading or its longest text. Description's is its narrowest;
    /// it takes the rest of the window.
    floors: [f32; Field::ALL.len()],
    /// The row whose Remove was pressed this frame.
    removed: Option<u64>,
}

impl GridSource for Rows<'_> {
    fn columns(&self) -> Vec<GridColumn> {
        let mut columns = vec![GridColumn::new("shown", tr("ui.drawing_revision_table.shown"), 0.6, CellKind::Check)];
        // The other texts are fixed at their content (weight 0: the larger of the heading and the
        // floor). Description is the one flexible column, so it takes all the width they leave.
        columns.extend(Field::ALL.iter().zip(&self.headings).zip(self.floors).map(|((field, heading), floor)| {
            let column = match field {
                Field::Description => GridColumn::new(field.key(), heading, 1.0, CellKind::Text).flex(),
                _ => GridColumn::new(field.key(), heading, 0.0, CellKind::Text),
            };
            column.min_content_width(floor).edit_on_click()
        }));
        columns.push(GridColumn::new("actions", "", 0.0, CellKind::Custom).min_content_width(ui_kit::table_row_height() * 1.2));
        columns
    }

    fn row_count(&self) -> usize {
        self.rows.len()
    }

    fn row_key(&self, row: usize) -> Cow<'_, str> {
        Cow::Owned(self.rows.get(row).map(|r| ids::encode(r.id)).unwrap_or_default())
    }

    fn text(&self, row: usize, col: usize) -> Cow<'_, str> {
        let Some(r) = self.rows.get(row) else { return Cow::Borrowed("") };
        match field(col) {
            Some(field) => Cow::Borrowed(r.field(field.key()).unwrap_or_default()),
            None if col == SHOWN => Cow::Borrowed(if r.hidden { "false" } else { "true" }),
            None => Cow::Borrowed(""),
        }
    }

    /// A text is checked by the record's own rule (a revision is one line) before it is sent.
    fn set_cell(&mut self, row: usize, col: usize, text: &str) -> Result<(), String> {
        let (Some(r), Some(field)) = (self.rows.get(row), field(col)) else { return Ok(()) };
        self.record.clone().set(r.id, field, text).map_err(|e| e.to_string())
    }

    fn add_row(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(tr("ui.drawing_revision_table.add_row")))
    }

    fn draw(&mut self, ui: &mut egui::Ui, row: usize, col: usize) -> Option<egui::Response> {
        let r = *self.rows.get(row)?;
        if col != ACTIONS {
            return None;
        }
        let remove = ui_kit::bin_button(ui, tr("ui.drawing_revision_table.remove"));
        crate::scenario::publish_handle(&format!("drawing:revision:remove:{}", ids::encode(r.id)), remove.rect.center());
        if remove.clicked() {
            self.removed = Some(r.id);
        }
        Some(remove)
    }
}

/// The commands that make a gesture's `edits`: per row, in the order the gesture first touched it,
/// one edit of all its changed fields, or one add with its texts for a new row.
fn steps(rows: &[&RevisionRow], edits: &[Edit]) -> Vec<(&'static str, Value)> {
    let mut touched: Vec<(RowRef, Map<String, Value>)> = Vec::new();
    for edit in edits {
        let value = match field(edit.col()) {
            Some(field) => (field.key(), json!(edit.text())),
            None if edit.col() == SHOWN => ("hidden", json!(edit.text() != "true")),
            None => continue,
        };
        match touched.iter_mut().find(|(row, _)| *row == *edit.row()) {
            Some((_, set)) => {
                set.insert(value.0.into(), value.1);
            }
            None => touched.push((edit.row().clone(), Map::from_iter([(value.0.into(), value.1)]))),
        }
    }
    touched
        .into_iter()
        .filter_map(|(row, set)| match row {
            RowRef::Existing(key) => {
                let id = ids::decode(&key).filter(|id| rows.iter().any(|r| r.id == *id))?;
                Some(("drawing.revision.edit", json!({"row": ids::encode(id), "set": set})))
            }
            RowRef::New(_) => Some(("drawing.revision.add", json!({"set": set}))),
        })
        .collect()
}

/// Whether a cell of the revision grid is being edited, as the kit's grid records it.
pub(super) fn editing(ctx: &egui::Context) -> bool {
    ui_kit::Grid::editing(ctx, ui_kit::Grid::state_of(GRID)).is_some()
}

/// The body for revision table `table`: its rows, then what the gesture made of them, run. Returns how
/// much wider its columns are than the room it had, for the window to grow by.
pub(super) fn editor(app: &mut SolveApp, ui: &mut egui::Ui, table: u64) -> f32 {
    let found = app.session.drawing("drawing.inspect").ok().and_then(|drawing| {
        let placed = drawing.tables.by_id(table).filter(|placed| matches!(placed.kind, Kind::Revision))?;
        let heading =
            |field: Field| placed.table.columns.iter().find(|c| c.name == field.key()).map_or(field.key(), |c| c.heading.as_str()).to_owned();
        Some((drawing.revisions.clone(), placed.sheet, Field::ALL.map(heading)))
    });
    // The window shows a table an undo took away as gone ([`super::table_window::show`]).
    let Some((record, sheet, headings)) = found else { return 0.0 };
    // A record kept per sheet lists each sheet's own rows.
    let scope = record.per_sheet.then_some(sheet);
    let rows: Vec<&RevisionRow> = record.rows().iter().filter(|r| r.sheet == scope).collect();
    // Each text column shows its heading and its longest text in full, so nothing is cut off; one
    // very long text stops at its widest and elides, so it can't push the other columns away.
    let card = solidcraft_theme::Tokens::get().form_card_width;
    let floors: [f32; Field::ALL.len()] = std::array::from_fn(|i| {
        let heading = headings.get(i).map_or(0.0, |heading| ui_kit::ContentWidth::label(ui, heading));
        let field = Field::ALL.get(i).copied();
        let key = field.map_or("", Field::key);
        let text = rows.iter().filter_map(|r| r.field(key)).map(|text| ui_kit::ContentWidth::cell_text(ui, text)).fold(0.0, f32::max);
        match field {
            Some(Field::Description) => heading.max(text.clamp(card * DESCRIPTION_CARDS.0, card * DESCRIPTION_CARDS.1)),
            _ => heading.max(text.min(TEXT_WIDEST)),
        }
    });
    let mut rows = Rows { record: &record, rows, headings, floors, removed: None };
    let out = ui_kit::Grid::new(GRID).empty(tr("ui.drawing_revision_table.empty")).show(ui, &mut rows);
    let overflow = out.overflow;
    let steps = steps(&rows.rows, &out.edits);
    let removed = rows.removed;
    let ran = if out.add_row {
        app.run("drawing.revision.add", json!({})).map(|_| ())
    } else if let Some(row) = removed {
        app.run("drawing.revision.delete", json!({"row": ids::encode(row)})).map(|_| ())
    } else if let [(command, params)] = &steps[..] {
        // One row: its answer may warn that an issued revision's copy keeps what it read.
        app.run(command, params.clone()).map(|reply| {
            let warnings: Vec<&str> = reply["warnings"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            if !warnings.is_empty() {
                app.set_status(warnings.join("; "), false);
            }
        })
    } else if steps.is_empty() {
        Ok(())
    } else {
        app.run_as_one(&steps)
    };
    if let Err(error) = ran {
        if !steps.is_empty() {
            ui_kit::Grid::refuse(ui.ctx(), ui_kit::Grid::state_of(GRID), &error);
        }
        app.set_status(error, true);
    } else if let Some(why) = out.refused {
        // A cell the record's rule refused before anything was sent.
        app.set_status(why, true);
    } else if let Some(notice) = out.notice {
        app.set_status(notice, false);
    }
    overflow
}
