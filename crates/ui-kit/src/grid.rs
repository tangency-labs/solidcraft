//! An editable grid over [`table_custom`]: one cell cursor, one editing model, ranges, TSV copy
//! and paste (as Excel and Google Sheets write it), fill down and clear, for every table.
//!
//! The grid keeps only view state (the cursor, the range, the edit buffer) in egui memory under
//! its id. The data stays with the caller's [`GridSource`], which never changes anything while
//! the grid draws: each user gesture (one commit, one paste, one fill, one clear) comes back from
//! [`Grid::show`] as ONE list of [`Edit`]s, for the caller to apply as one command and one undo
//! step. When that command is refused, [`Grid::refuse`] puts the edit back.
//!
//! Keys, with no cell being edited: the arrows move the cursor and Shift+arrows extend the range;
//! Tab and Shift+Tab move across a row and wrap to the next; Enter and Shift+Enter move down and
//! up the column; Home and End go to the row's ends, Ctrl+Home and Ctrl+End to the table's.
//! Typing starts an edit that replaces the cell; F2 or a double-click starts one with all its
//! text selected. While editing: Escape cancels; Enter, Tab and leaving the cell commit, then
//! move as above; Ctrl+Enter writes the value to every selected cell. Ctrl+C copies the range,
//! Ctrl+V pastes (one value over a range fills it), Ctrl+D fills the top row down, Delete and
//! Backspace clear, Ctrl+A selects everything.
//!
//! Rows have one fixed height ([`table_row_height`]) and only the rows on screen are built,
//! with one spacer above and one below, so a grid of 10,000 rows costs what its visible rows do.

use std::borrow::Cow;

use crate::BackingTooltipExt as _;
use egui::{Event, Key, Response, Ui, vec2};
use solidcraft_theme::Tokens;

use super::grid_tsv::{read_tsv, write_tsv};
use super::metrics::table_row_height;
use super::{Column, Table, table_custom};

/// What a cell holds, which decides how it is drawn, edited and pasted into.
#[derive(Clone, Debug, PartialEq)]
pub enum CellKind {
    /// Free text, edited in place.
    Text,
    /// A number written with its unit ("20 mm"), right-aligned; the edit text keeps the unit.
    Number { unit: String },
    /// A formula in the host's expression language, with its completion
    /// ([`crate::install_expressions`]).
    Expression,
    /// One of these options, from a dropdown. A paste or edit must name one of them.
    Choice(Vec<String>),
    /// A check box; its text is `true` or `false`.
    Check,
    /// Shown and copied, never changed from the grid.
    ReadOnly,
    /// Drawn by the source ([`GridSource::draw`]): buttons, pickers. Copied as its text.
    Custom,
}

impl CellKind {
    /// Whether the cell edits as text in a box.
    fn types(&self) -> bool {
        matches!(self, CellKind::Text | CellKind::Number { .. } | CellKind::Expression)
    }

    /// Whether a paste, fill or clear may write to the cell.
    fn writable(&self) -> bool {
        !matches!(self, CellKind::ReadOnly | CellKind::Custom)
    }
}

/// One column of a grid: a stable key (for handles), its heading, its layout as a
/// [`Column`] and the kind of its cells.
#[derive(Clone, Debug)]
pub struct GridColumn {
    key: String,
    label: String,
    weight: f32,
    flex: bool,
    min_content_width: f32,
    icon_only: bool,
    kind: CellKind,
    edit_on_click: bool,
}

impl GridColumn {
    /// A column named `label` (translated when drawn), its width weighted as [`Column::new`].
    pub fn new(key: &str, label: &str, weight: f32, kind: CellKind) -> Self {
        GridColumn {
            key: key.to_owned(),
            label: label.to_owned(),
            weight,
            flex: false,
            min_content_width: 0.0,
            icon_only: false,
            kind,
            edit_on_click: false,
        }
    }

    /// Share the remaining width, as [`Column::flex`].
    pub fn flex(mut self) -> Self {
        self.flex = true;
        self
    }

    /// Keep room for the cell's controls, as [`Column::min_content_width`].
    pub fn min_content_width(mut self, width: f32) -> Self {
        self.min_content_width = width;
        self
    }

    /// No visible heading, as [`Column::icon_only`].
    pub fn icon_only(mut self) -> Self {
        self.icon_only = true;
        self
    }

    /// A single click edits the cell with the caret where it landed, as a form field does,
    /// instead of only moving the cursor there. For tables whose cells were always-on boxes.
    pub fn edit_on_click(mut self) -> Self {
        self.edit_on_click = true;
        self
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    fn layout(&self, manual: Option<f32>) -> Column<'_> {
        let mut column = Column::new(&self.label, self.weight).min_content_width(self.min_content_width).manual_width(manual);
        if self.flex {
            column = column.flex();
        }
        if self.icon_only {
            column = column.icon_only();
        }
        column
    }
}

/// The row an edit is for: one the source has (by its key), or the `n`th new row a paste below
/// the last row asks for (only when [`GridSource::add_row`] offers rows).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowRef {
    Existing(String),
    New(usize),
}

/// One change to one cell. Columns are indices into [`GridSource::columns`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    Set { row: RowRef, col: usize, text: String },
    Clear { row: RowRef, col: usize },
}

impl Edit {
    pub fn row(&self) -> &RowRef {
        match self {
            Edit::Set { row, .. } | Edit::Clear { row, .. } => row,
        }
    }

    pub fn col(&self) -> usize {
        match self {
            Edit::Set { col, .. } | Edit::Clear { col, .. } => *col,
        }
    }

    /// The text the cell is to hold: empty for a clear.
    pub fn text(&self) -> &str {
        match self {
            Edit::Set { text, .. } => text,
            Edit::Clear { .. } => "",
        }
    }
}

/// The gesture an edit list came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    /// One cell's edit committed (Enter, Tab, leaving it, a dropdown pick, a check).
    Commit,
    /// One value written to every selected cell (Ctrl+Enter).
    CommitAll,
    Paste,
    Fill,
    Clear,
}

/// What the user did this frame.
#[derive(Debug, Default)]
pub struct GridOutput {
    /// The edits of one gesture, to apply as one command; empty when nothing changed.
    pub edits: Vec<Edit>,
    pub gesture: Option<Gesture>,
    /// Whether the gesture was committed by a key (Enter, Tab), not by leaving the cell.
    pub by_key: bool,
    /// Why the source refused a cell of this gesture ([`GridSource::set_cell`]), if it did.
    pub refused: Option<String>,
    /// A note about the gesture, such as a paste cut off at the last row.
    pub notice: Option<String>,
    /// The cell being edited and its text as typed so far: row key, column, text.
    pub editing: Option<(String, usize, String)>,
    /// An edit cancelled with Escape this frame: row key, column.
    pub cancelled: Option<(String, usize)>,
    /// The add-row control was pressed: the source adds its draft row.
    pub add_row: bool,
}

/// What [`GridSource::editor`] drew.
pub struct CellEditor {
    pub response: Response,
    /// The editor took this frame's keys itself (a completion was accepted): nothing commits.
    pub taken: bool,
}

/// The data behind a grid. The grid only reads it while drawing; changes come back as edits.
pub trait GridSource {
    /// The columns, in order. Called once per frame.
    fn columns(&self) -> Vec<GridColumn>;
    fn row_count(&self) -> usize;
    /// A key for the row that stays the same across sorting and filtering.
    fn row_key(&self, row: usize) -> Cow<'_, str>;
    /// The cell's text: shown, copied and edited.
    fn text(&self, row: usize, col: usize) -> Cow<'_, str>;
    /// This cell's kind when it differs from its column's (a read-only cell in an editable column).
    fn kind(&self, _row: usize, _col: usize) -> Option<CellKind> {
        None
    }
    /// A message to show on the cell: its text and whether it is an error.
    fn message(&self, _row: usize, _col: usize) -> Option<(Cow<'_, str>, bool)> {
        None
    }
    /// Whether the cell may take `text`, checked before the grid hands the edit back: `Err` holds
    /// the refusal shown to the user. It must not change anything; the edit list does that.
    fn set_cell(&mut self, _row: usize, _col: usize, _text: &str) -> Result<(), String> {
        Ok(())
    }
    /// The label of the add-row control under the last row, when the source can add rows. A paste
    /// longer than the rows below the cursor then asks for new rows ([`RowRef::New`]).
    fn add_row(&self) -> Option<Cow<'_, str>> {
        None
    }
    /// Whether the last heading holds the add-column control, which opens [`Self::add_column`].
    fn can_add_column(&self) -> bool {
        false
    }
    /// The add-column control's menu.
    fn add_column(&mut self, _ui: &mut Ui) {}
    /// Draw the heading of `col` instead of its label (a sort control); true when it did.
    fn header(&mut self, _ui: &mut Ui, _col: usize) -> bool {
        false
    }
    /// Draw a cell that is not being edited, instead of the grid's own text: a button, a name with
    /// its hint. `None` leaves the cell to the grid. Always used for [`CellKind::Custom`].
    fn draw(&mut self, _ui: &mut Ui, _row: usize, _col: usize) -> Option<Response> {
        None
    }
    /// Draw the box a cell is edited in; `id` is the box's widget id. The default is the kit's
    /// [`crate::cell_editor`], with the host's completion for expressions.
    fn editor(&mut self, ui: &mut Ui, row: usize, col: usize, id: egui::Id, text: &mut String) -> CellEditor {
        let invalid = self.message(row, col).is_some_and(|(_, error)| error);
        let expression = matches!(self.kind(row, col), Some(CellKind::Expression));
        let response = crate::cell_editor(ui, id, text, "", invalid);
        let taken = expression && crate::expressions(ui.ctx()).is_some_and(|e| e.complete(ui, &response, text));
        CellEditor { response, taken }
    }
    /// Called after each built cell is drawn, with the cell's response (the editor's while it is
    /// edited), so the source can publish its own handles for it.
    fn drawn(&mut self, _ui: &Ui, _row: usize, _col: usize, _response: &Response, _editing: bool) {}
}

/// Where the cursor or the range's anchor is: the row by key, and by its place among the shown
/// rows for when the key is gone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Spot {
    key: String,
    row: usize,
    col: usize,
}

/// Where the caret goes when an edit opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Caret {
    /// All the text selected (F2, double-click).
    All,
    /// After the text (typing started the edit).
    End,
    /// At this character (a click).
    At(usize),
}

#[derive(Clone, Debug)]
struct Editing {
    at: Spot,
    text: String,
    /// Set until the editor first draws.
    caret: Option<Caret>,
    /// Frames the editor has been drawn without the keyboard, after having had it.
    unfocused: u8,
    had_focus: bool,
    /// Why the last commit of this edit was refused.
    message: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct State {
    cursor: Option<Spot>,
    anchor: Option<Spot>,
    edit: Option<Editing>,
    dragging: bool,
    /// Cells a gesture could not write, with why: row key, column, message.
    refused: Vec<(String, usize, String)>,
    /// The last single-cell commit (where, its text, by key), for [`Grid::refuse`].
    last: Option<(Spot, String, bool)>,
    /// Take the keyboard (after a keyboard commit, or [`Grid::refuse`]).
    focus: bool,
    /// Scroll the cursor into view.
    reveal: bool,
    /// A keyboard commit's move (rows, columns, wrapping), made once every cell is drawn.
    pending_move: Option<(isize, isize, bool)>,
    /// A Ctrl+Enter commit's text, written to the range once every cell is drawn.
    pending_all: Option<String>,
    /// The grid had the keyboard at the end of the last frame, and an arrow or Tab came in it.
    held: bool,
    keyed: bool,
}

/// The keys that stay with a grid holding the keyboard, rather than moving egui's focus.
const KEYS_STAY: egui::EventFilter = egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: false };

/// Give the grid's focus target the keyboard when the grid asked for it (a click on a cell, a
/// keyboard commit), unless the pointer is pressed or released this frame. The filter goes with
/// the focus at once: egui's own arrow and Tab navigation runs before the grid draws again.
fn take_focus(ctx: &egui::Context, state: &mut State, focus: egui::Id) {
    if !state.focus || state.edit.is_some() || ctx.input(|i| i.pointer.any_pressed() || i.pointer.any_released()) {
        return;
    }
    ctx.memory_mut(|m| {
        m.request_focus(focus);
        m.set_focus_lock_filter(focus, KEYS_STAY);
    });
    state.focus = false;
}

/// The editable grid. Build it per frame, then [`Grid::show`] it.
pub struct Grid<'a> {
    id: &'a str,
    state: Option<egui::Id>,
    empty: &'a str,
    resize: Option<egui::Id>,
    filter: Option<&'a dyn Fn(usize) -> bool>,
}

fn focus_id(state: egui::Id) -> egui::Id {
    state.with("focus")
}

fn editor_id(state: egui::Id, row: &str, col: usize) -> egui::Id {
    state.with(("edit", row, col))
}

fn load(ctx: &egui::Context, id: egui::Id) -> State {
    ctx.data(|d| d.get_temp::<State>(id)).unwrap_or_default()
}

fn store(ctx: &egui::Context, id: egui::Id, state: State) {
    ctx.data_mut(|d| d.insert_temp(id, state));
}

/// The rows the grid shows, in order: all of the source's, or those the filter keeps.
struct Shown(Option<Vec<usize>>, usize);

impl Shown {
    fn len(&self) -> usize {
        self.0.as_ref().map_or(self.1, Vec::len)
    }

    fn source(&self, row: usize) -> Option<usize> {
        match &self.0 {
            Some(rows) => rows.get(row).copied(),
            None => (row < self.1).then_some(row),
        }
    }
}

/// A rectangle of cells among the shown rows, inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Range {
    rows: (usize, usize),
    cols: (usize, usize),
}

impl Range {
    fn contains(&self, row: usize, col: usize) -> bool {
        (self.rows.0..=self.rows.1).contains(&row) && (self.cols.0..=self.cols.1).contains(&col)
    }

    fn single(&self) -> bool {
        self.rows.0 == self.rows.1 && self.cols.0 == self.cols.1
    }
}

impl<'a> Grid<'a> {
    /// A grid whose state and handles go by `id` (as [`table_custom`]'s: its rows publish
    /// `dialog:table:{id}:{row key}`, and each cell `grid:{id}:{row key}:{column key}`).
    pub fn new(id: &'a str) -> Self {
        Grid { id, state: None, empty: "", resize: None, filter: None }
    }

    /// Where the grid keeps its view state when not under [`Grid::state_of`] its id: a window
    /// that opens afresh passes a new one, so an edit left open does not come back with it.
    pub fn state(mut self, id: egui::Id) -> Self {
        self.state = Some(id);
        self
    }

    /// The state id of a grid made with `id` and no [`Grid::state`], for [`Grid::refuse`] and the
    /// other functions that reach a grid between frames.
    pub fn state_of(id: &str) -> egui::Id {
        egui::Id::unique(("sc_grid", id))
    }

    /// What the grid says when it has no rows.
    pub fn empty(mut self, text: &'a str) -> Self {
        self.empty = text;
        self
    }

    /// Draggable column dividers, the widths kept under `key` ([`Table::headers_resizable`]).
    pub fn resizable(mut self, key: egui::Id) -> Self {
        self.resize = Some(key);
        self
    }

    /// Show only the source rows `keep` is true for; the cursor moves among those.
    pub fn filter(mut self, keep: &'a dyn Fn(usize) -> bool) -> Self {
        self.filter = Some(keep);
        self
    }

    /// Draw the grid. See [`Grid::show_with`].
    pub fn show<S: GridSource>(self, ui: &mut Ui, source: &mut S) -> GridOutput {
        self.show_with(ui, source, |_, _, _| {})
    }

    /// Draw the grid, with `footer` drawing extra rows (a draft row) inside the table after the
    /// last row and before the add-row control.
    pub fn show_with<S: GridSource>(self, ui: &mut Ui, source: &mut S, footer: impl FnOnce(&mut Ui, &mut Table<'_>, &mut S)) -> GridOutput {
        let id = self.id;
        let sid = self.state.unwrap_or_else(|| Grid::state_of(id));
        let ctx = ui.ctx().clone();
        let mut state = load(&ctx, sid);
        let mut out = GridOutput::default();
        let columns = source.columns();
        let mut extra = columns.len();
        let adds_column = source.can_add_column();
        let count = source.row_count();
        let shown = Shown(self.filter.map(|keep| (0..count).filter(|r| keep(*r)).collect()), count);
        resolve(&mut state, source, &shown, columns.len());
        let focus = focus_id(sid);
        // A press elsewhere takes egui's focus away at the end of its frame, so the grid takes the
        // keyboard it asked for on a later frame without one, before reading this frame's keys.
        // egui keeps a widget's key filter only from its second frame with the keyboard, so on the
        // first one its own arrow or Tab navigation can take the keyboard away: take it back.
        state.focus |= state.held && state.keyed && state.edit.is_none() && !ctx.memory(|m| m.has_focus(focus));
        take_focus(&ctx, &mut state, focus);
        let editing_focused = state.edit.as_ref().is_some_and(|e| ctx.memory(|m| m.has_focus(editor_id(sid, &e.at.key, e.at.col))));
        let focused = ctx.memory(|m| m.has_focus(focus));
        // The grid's keys are its own: egui must not also move its focus on an arrow or Tab
        // (its key filter only holds from a widget's second frame with the keyboard).
        if (focused || editing_focused)
            && ctx.input(|i| {
                i.raw.events.iter().any(|e| {
                    matches!(e, Event::Key { key: Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight | Key::Tab, pressed: true, .. })
                })
            })
        {
            ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
        }
        if focused && state.edit.is_none() && !egui::Popup::is_any_open(&ctx) {
            navigate(ui, &mut state, source, &shown, &columns, &mut out);
        }
        let manual: Vec<Option<f32>> = self.resize.and_then(|key| ctx.data_mut(|m| m.get_persisted(key))).unwrap_or_default();
        let mut layout: Vec<Column<'_>> = columns.iter().enumerate().map(|(i, c)| c.layout(manual.get(i).copied().flatten())).collect();
        if adds_column {
            layout.push(Column::new("", 0.0).icon_only().min_content_width(Tokens::get().form_close_size));
            extra = layout.len() - 1;
        }
        let range = range(&state, &shown, columns.len());
        let shows_focus = focused || editing_focused || state.focus;
        let mut clicks: Vec<Click> = Vec::new();
        let mut cells: Vec<(usize, usize, egui::Rect)> = Vec::new();
        let resize = self.resize;
        table_custom(ui, id, &layout, self.empty, |ui, table| {
            let header = |ui: &mut Ui, index: usize| {
                if index == extra && adds_column {
                    super::plus_header(ui, |ui| source.add_column(ui));
                    return true;
                }
                source.header(ui, index)
            };
            match resize {
                Some(key) => table.headers_resizable(ui, key, header),
                None => table.headers(ui, header),
            }
            // The focus target that holds the keyboard while no cell is edited. Arrows and Tab stay
            // with it rather than egui's focus order.
            let target = ui.interact(egui::Rect::from_min_size(ui.cursor().min, vec2(1.0, 1.0)), focus, egui::Sense::focusable_noninteractive());
            if target.has_focus() {
                ui.memory_mut(|m| m.set_focus_lock_filter(focus, KEYS_STAY));
            }
            let height = table_row_height();
            let top = ui.cursor().top();
            let clip = ui.clip_rect();
            let rows = shown.len();
            let first = (((clip.top() - top) / height).floor().max(0.0) as usize).min(rows);
            let last = (((clip.bottom() - top) / height).ceil().max(0.0) as usize).min(rows);
            // Rows built off screen: the one being edited (its box keeps the keyboard and commits on
            // leaving) and the cursor's when it must scroll into view.
            let mut built: Vec<usize> = (first..last).collect();
            for spot in [state.edit.as_ref().map(|e| &e.at), state.reveal.then_some(state.cursor.as_ref()).flatten()].into_iter().flatten() {
                if spot.row < rows && !built.contains(&spot.row) {
                    built.push(spot.row);
                }
            }
            built.sort_unstable();
            let mut next = 0;
            for row in built {
                if row > next {
                    table.skip_rows(ui, row - next);
                }
                next = row + 1;
                let Some(source_row) = shown.source(row) else { continue };
                let key = source.row_key(source_row).into_owned();
                let mut reveal = None;
                table.row_visible(ui, &key, false, true, |table_row| {
                    for (col, column) in columns.iter().enumerate() {
                        table_row.cell(|ui| {
                            let rect = ui.max_rect();
                            cells.push((row, col, rect));
                            let cell = Cell { id, sid, row, source_row, col, key: &key, column, rect };
                            draw_cell(ui, &cell, &mut state, source, &shown, range, shows_focus, &mut clicks, &mut out);
                            if state.reveal && state.cursor.as_ref().is_some_and(|c| c.row == row && c.col == col) {
                                reveal = Some(rect);
                            }
                        });
                    }
                    if adds_column {
                        table_row.cell(|_| {});
                    }
                });
                if let Some(rect) = reveal {
                    table.reveal_row(rect);
                    state.reveal = false;
                }
            }
            if rows > next {
                table.skip_rows(ui, rows - next);
            }
            footer(ui, table, source);
            if let Some(label) = source.add_row() {
                let label = label.into_owned();
                if table.add_prompt(ui, &label, &label).clicked() {
                    out.add_row = true;
                }
            }
        });
        pointer(ui, &mut state, source, &shown, &columns, &cells, clicks, &mut out);
        take_focus(&ctx, &mut state, focus);
        state.held = ctx.memory(|m| m.has_focus(focus));
        state.keyed = ctx.input(|i| {
            i.raw.events.iter().any(|e| {
                matches!(e, Event::Key { key: Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight | Key::Tab, pressed: true, .. })
            })
        });
        if let Some(edit) = &state.edit {
            out.editing = Some((edit.at.key.clone(), edit.at.col, edit.text.clone()));
        }
        store(&ctx, sid, state);
        out
    }

    /// The caller's command for the last gesture was refused with `message`. A single cell
    /// committed by a key opens again with the text that was refused, the message beside it; a
    /// cell committed by leaving it keeps its value and shows the message.
    pub fn refuse(ctx: &egui::Context, id: egui::Id, message: &str) {
        let mut state = load(ctx, id);
        if let Some((at, text, by_key)) = state.last.take() {
            if by_key {
                state.cursor = Some(at.clone());
                state.anchor = Some(at.clone());
                state.edit = Some(Editing { at, text, caret: Some(Caret::End), unfocused: 0, had_focus: false, message: Some(message.to_owned()) });
            } else {
                state.refused.push((at.key, at.col, message.to_owned()));
            }
        }
        store(ctx, id, state);
    }

    /// Forget grid `id`'s view state: its cursor, range and any open edit.
    pub fn forget(ctx: &egui::Context, id: egui::Id) {
        ctx.data_mut(|d| d.remove_temp::<State>(id));
    }

    /// The cell being edited in grid `id`, if any: row key, column, text as typed.
    pub fn editing(ctx: &egui::Context, id: egui::Id) -> Option<(String, usize, String)> {
        load(ctx, id).edit.map(|e| (e.at.key, e.at.col, e.text))
    }

    /// Put the cursor of grid `id` on a cell (and the range on it alone), as a click does.
    pub fn select(ctx: &egui::Context, id: egui::Id, row_key: &str, row: usize, col: usize) {
        let mut state = load(ctx, id);
        let spot = Spot { key: row_key.to_owned(), row, col };
        state.cursor = Some(spot.clone());
        state.anchor = Some(spot);
        state.edit = None;
        state.focus = true;
        store(ctx, id, state);
    }

    /// The selected range of grid `id` as (first row, last row, first column, last column) among
    /// the shown rows, if it has a cursor.
    pub fn selection(ctx: &egui::Context, id: egui::Id) -> Option<(usize, usize, usize, usize)> {
        let state = load(ctx, id);
        let (cursor, anchor) = (state.cursor?, state.anchor?);
        Some((cursor.row.min(anchor.row), cursor.row.max(anchor.row), cursor.col.min(anchor.col), cursor.col.max(anchor.col)))
    }
}

/// Move the cursor and anchor onto rows that still exist: by key, else the nearest place.
fn resolve<S: GridSource>(state: &mut State, source: &S, shown: &Shown, cols: usize) {
    let rows = shown.len();
    let find = |spot: &mut Spot| -> bool {
        if rows == 0 || cols == 0 {
            return false;
        }
        spot.col = spot.col.min(cols - 1);
        let key_at = |row: usize| shown.source(row).map(|r| source.row_key(r));
        if spot.row < rows && key_at(spot.row).is_some_and(|k| k == spot.key.as_str()) {
            return true;
        }
        if let Some(row) = (0..rows).find(|r| key_at(*r).is_some_and(|k| k == spot.key.as_str())) {
            spot.row = row;
            return true;
        }
        // The row is gone (deleted, renamed or filtered out): the nearest row that is left.
        spot.row = spot.row.min(rows - 1);
        spot.key = key_at(spot.row).map(Cow::into_owned).unwrap_or_default();
        true
    };
    if let Some(cursor) = state.cursor.as_mut()
        && !find(cursor)
    {
        state.cursor = None;
    }
    if let Some(anchor) = state.anchor.as_mut()
        && !find(anchor)
    {
        state.anchor = None;
    }
    if let Some(edit) = state.edit.as_mut() {
        let key = edit.at.key.clone();
        if !find(&mut edit.at) || edit.at.key != key {
            state.edit = None;
        }
    }
    if state.cursor.is_none() {
        state.anchor = None;
    }
}

fn range(state: &State, shown: &Shown, cols: usize) -> Option<Range> {
    let cursor = state.cursor.as_ref()?;
    let anchor = state.anchor.as_ref().unwrap_or(cursor);
    if shown.len() == 0 || cols == 0 {
        return None;
    }
    Some(Range { rows: (cursor.row.min(anchor.row), cursor.row.max(anchor.row)), cols: (cursor.col.min(anchor.col), cursor.col.max(anchor.col)) })
}

/// One cell being drawn.
struct Cell<'c> {
    id: &'c str,
    sid: egui::Id,
    row: usize,
    source_row: usize,
    col: usize,
    key: &'c str,
    column: &'c GridColumn,
    rect: egui::Rect,
}

/// A pointer press or click on a cell, handled after every cell is drawn (an open editor
/// commits first, on losing the keyboard).
#[derive(Clone, Copy)]
enum Click {
    Press { row: usize, col: usize, shift: bool },
    Edit { row: usize, col: usize, caret: Caret },
}

fn kind_of<S: GridSource>(source: &S, columns: &[GridColumn], row: usize, col: usize) -> CellKind {
    source.kind(row, col).or_else(|| columns.get(col).map(|c| c.kind.clone())).unwrap_or(CellKind::ReadOnly)
}

fn checked(text: &str) -> bool {
    matches!(text.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes")
}

fn draw_cell<S: GridSource>(
    ui: &mut Ui,
    cell: &Cell<'_>,
    state: &mut State,
    source: &mut S,
    shown: &Shown,
    range: Option<Range>,
    shows_focus: bool,
    clicks: &mut Vec<Click>,
    out: &mut GridOutput,
) {
    let t = Tokens::get();
    let (row, col, rect) = (cell.row, cell.col, cell.rect);
    let kind = source.kind(cell.source_row, col).unwrap_or_else(|| cell.column.kind.clone());
    let editing = state.edit.as_ref().is_some_and(|e| e.at.row == row && e.at.col == col);
    let in_range = range.is_some_and(|r| r.contains(row, col) && !r.single());
    let is_cursor = state.cursor.as_ref().is_some_and(|c| c.row == row && c.col == col);
    if in_range && shows_focus {
        ui.painter().rect_filled(rect, 0.0, t.selected_bg);
    }
    let name = cell_name(cell);
    let handle = format!("grid:{}:{}:{}", cell.id, cell.key, cell.column.key);
    if editing {
        edit_cell(ui, cell, state, source, shown, out);
        return;
    }
    let text = source.text(cell.source_row, col).into_owned();
    let message = source.message(cell.source_row, col).map(|(m, e)| (m.into_owned(), e));
    let refused = state.refused.iter().find(|(k, c, _)| k == cell.key && *c == col).map(|(_, _, m)| m.clone());
    let mut response = ui.interact(rect, cell.sid.with(("cell", cell.key, col)), egui::Sense::click_and_drag());
    let error = message.as_ref().is_some_and(|(_, e)| *e) || refused.is_some();
    // What the cell shows: the source's own drawing, a control, or its text.
    let drawn = match &kind {
        CellKind::Choice(options) => {
            let labels: Vec<&str> = options.iter().map(String::as_str).collect();
            let mut selected = options.iter().position(|o| *o == text).unwrap_or(0);
            let before = selected;
            let shown = crate::dropdown(ui, &format!("{handle}:choice"), &mut selected, &labels);
            if selected != before
                && let Some(option) = options.get(selected)
            {
                single(cell, state, source, option, Gesture::Commit, out);
            }
            Some(shown)
        }
        CellKind::Check => {
            let mut value = checked(&text);
            let shown = crate::checkbox(ui, "", &mut value);
            if shown.changed() {
                single(cell, state, source, if value { "true" } else { "false" }, Gesture::Commit, out);
            }
            Some(shown)
        }
        _ => source.draw(ui, cell.source_row, col),
    };
    if drawn.is_none() {
        let inset = t.form_inset;
        if kind.types() {
            ui.painter().rect_filled(rect.shrink2(vec2(0.0, (rect.height() - t.control_height).max(0.0) / 2.0)), t.control_radius, t.input_fill());
        }
        let ink = if error {
            t.form_error
        } else if kind.writable() {
            t.form_ink
        } else {
            t.form_dim
        };
        let font = egui::FontId::monospace(t.font_label);
        let text_rect = rect.shrink2(vec2(inset, 0.0));
        if matches!(kind, CellKind::Number { .. }) {
            let width = ui.painter().layout_no_wrap(text.clone(), font.clone(), ink).size().x.min(text_rect.width());
            let right = egui::Rect::from_min_max(egui::pos2(text_rect.right() - width, text_rect.top()), text_rect.max);
            super::text::paint_text(ui, right, &text, font, ink, false);
        } else {
            super::text::paint_text(ui, text_rect, &text, font, ink, false);
        }
    }
    if let Some(drawn) = &drawn {
        response = response.union(drawn.clone());
    }
    if let Some(message) = refused.as_ref().or(message.as_ref().map(|(m, _)| m)) {
        response = response.backing_on_hover_text(message.clone());
    }
    if error {
        ui.painter().rect_stroke(rect.shrink(0.5), t.control_radius, egui::Stroke::new(1.0, t.form_error), egui::StrokeKind::Inside);
    }
    if is_cursor && shows_focus {
        ui.painter().rect_stroke(rect, t.control_radius, egui::Stroke::new(t.space_2, t.accent), egui::StrokeKind::Inside);
    }
    let enabled = ui.is_enabled();
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::Role::GridCell, enabled, &name);
        info.current_text_value = Some(text.clone());
        info.selected = Some(is_cursor || in_range);
        info
    });
    solidcraft_ui_registry::publish_target(&handle, rect);
    // The press's own modifiers: a click and its Shift arrive in the same frame.
    let shift = ui.input(|i| {
        i.events.iter().find_map(|e| match e {
            Event::PointerButton { pressed: true, modifiers, .. } => Some(modifiers.shift),
            _ => None,
        })
    });
    // A press and its release can come in one frame: the click counts as the press too.
    let pressed_here =
        (response.is_pointer_button_down_on() || response.clicked() || response.drag_started()) && ui.input(|i| i.pointer.primary_pressed());
    if pressed_here {
        clicks.push(Click::Press { row, col, shift: shift.unwrap_or(false) });
    }
    let shift = shift.unwrap_or_else(|| ui.input(|i| i.modifiers.shift));
    if !shift && kind.types() {
        if response.double_clicked() {
            clicks.push(Click::Edit { row, col, caret: Caret::All });
        } else if response.clicked() && cell.column.edit_on_click {
            let caret = response.interact_pointer_pos().map_or(Caret::End, |pos| Caret::At(caret_at(ui, &text, rect, pos)));
            clicks.push(Click::Edit { row, col, caret });
        }
    }
    source.drawn(ui, cell.source_row, col, &response, false);
}

/// A cell's accessible name: "{column}, {row key}" (the column's key when it has no heading).
fn cell_name(cell: &Cell<'_>) -> String {
    let column = if cell.column.label.is_empty() { cell.column.key.as_str() } else { solidcraft_ui_text::tr_label(&cell.column.label) };
    format!("{column}, {}", cell.key)
}

/// The character a click at `pos` lands before, in a cell showing `text` from `rect`'s left inset.
fn caret_at(ui: &Ui, text: &str, rect: egui::Rect, pos: egui::Pos2) -> usize {
    let t = Tokens::get();
    let galley = ui.painter().layout_no_wrap(text.to_owned(), egui::FontId::monospace(t.font_label), t.form_ink);
    galley.cursor_from_pos(vec2(pos.x - rect.left() - t.form_inset, galley.size().y / 2.0)).index.0
}

/// Hand back one cell's value as a gesture of its own (a dropdown pick, a check), unless the
/// source refuses it.
fn single<S: GridSource>(cell: &Cell<'_>, state: &mut State, source: &mut S, text: &str, gesture: Gesture, out: &mut GridOutput) {
    let spot = Spot { key: cell.key.to_owned(), row: cell.row, col: cell.col };
    state.cursor = Some(spot.clone());
    state.anchor = Some(spot.clone());
    match source.set_cell(cell.source_row, cell.col, text) {
        Ok(()) => {
            state.last = Some((spot, text.to_owned(), false));
            out.edits = vec![Edit::Set { row: RowRef::Existing(cell.key.to_owned()), col: cell.col, text: text.to_owned() }];
            out.gesture = Some(gesture);
        }
        Err(message) => {
            state.refused.push((cell.key.to_owned(), cell.col, message.clone()));
            out.refused = Some(message);
        }
    }
}

/// The cell being edited: its box, and the keys and focus changes that end the edit.
fn edit_cell<S: GridSource>(ui: &mut Ui, cell: &Cell<'_>, state: &mut State, source: &mut S, shown: &Shown, out: &mut GridOutput) {
    let t = Tokens::get();
    let id = editor_id(cell.sid, cell.key, cell.col);
    let Some(edit) = state.edit.as_mut() else { return };
    if let Some(caret) = edit.caret.take() {
        ui.memory_mut(|m| m.request_focus(id));
        let chars = edit.text.chars().count();
        let (from, to) = match caret {
            Caret::All => (0, chars),
            Caret::End => (chars, chars),
            Caret::At(at) => (at.min(chars), at.min(chars)),
        };
        let mut text_state = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
        text_state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(from), egui::text::CCursor::new(to))));
        text_state.store(ui.ctx(), id);
    }
    let mut text = std::mem::take(&mut edit.text);
    let editor = source.editor(ui, cell.source_row, cell.col, id, &mut text);
    crate::keep_tab_in_cell(ui, &editor.response);
    let Some(edit) = state.edit.as_mut() else { return };
    edit.text = text;
    let has_focus = ui.memory(|m| m.has_focus(id));
    let focused = has_focus || ui.memory(|m| m.had_focus_last_frame(id));
    if has_focus {
        edit.had_focus = true;
        edit.unfocused = 0;
    } else if edit.had_focus {
        edit.unfocused = edit.unfocused.saturating_add(1);
    }
    if let Some(message) = &edit.message {
        ui.painter().rect_stroke(cell.rect, t.control_radius, egui::Stroke::new(1.0, t.form_error), egui::StrokeKind::Outside);
        let _ = editor.response.clone().backing_on_hover_text(message.clone());
    }
    ui.painter().rect_stroke(cell.rect, t.control_radius, egui::Stroke::new(t.space_2, t.accent), egui::StrokeKind::Outside);
    let name = cell_name(cell);
    solidcraft_ui_registry::publish_target(&format!("grid:{}:{}:{}", cell.id, cell.key, cell.column.key), editor.response.rect);
    editor.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, &name));
    source.drawn(ui, cell.source_row, cell.col, &editor.response, true);
    if editor.taken {
        return;
    }
    let escape = focused && ui.input(|i| i.raw.events.iter().any(|e| matches!(e, Event::Key { key: Key::Escape, pressed: true, .. })));
    if escape {
        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        out.cancelled = Some((cell.key.to_owned(), cell.col));
        state.edit = None;
        state.focus = true;
        return;
    }
    let all = focused
        && ui.input(|i| {
            i.raw.events.iter().any(
                |e| matches!(e, Event::Key { key: Key::Enter, pressed: true, repeat: false, modifiers, .. } if modifiers.command && !modifiers.shift),
            )
        });
    let key = crate::cell_key(ui, focused);
    let left = editor.response.lost_focus() || (edit.had_focus && edit.unfocused >= 2);
    if !all && key.is_none() && !left {
        return;
    }
    if key.is_some() || all {
        ui.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, Key::Enter);
            i.consume_key(egui::Modifiers::SHIFT, Key::Enter);
            i.consume_key(egui::Modifiers::COMMAND, Key::Enter);
        });
    }
    commit(state, source, shown, key, all, out);
}

/// End the open edit: hand back its edit (or, with `all`, one for every writable selected cell),
/// then move as `key` says. A refusal by a key keeps the edit open with its reason; a refusal on
/// leaving the cell ends it and shows the reason on the cell.
fn commit<S: GridSource>(state: &mut State, source: &mut S, shown: &Shown, key: Option<crate::CellKey>, all: bool, out: &mut GridOutput) {
    let Some(edit) = state.edit.clone() else { return };
    let Some(source_row) = shown.source(edit.at.row) else {
        state.edit = None;
        return;
    };
    let (row_key, col) = (edit.at.key.clone(), edit.at.col);
    let by_key = key.is_some() || all;
    if all {
        // Every selected cell, written once the cells are drawn (the range may reach rows that
        // were not built this frame).
        state.edit = None;
        state.focus = true;
        state.last = None;
        out.by_key = true;
        out.gesture = Some(Gesture::CommitAll);
        state.pending_all = Some(edit.text);
        return;
    }
    if edit.text == source.text(source_row, col) {
        state.edit = None;
    } else {
        match source.set_cell(source_row, col, &edit.text) {
            Ok(()) => {
                state.refused.retain(|(k, c, _)| !(*k == row_key && *c == col));
                state.last = Some((edit.at.clone(), edit.text.clone(), by_key));
                out.edits = vec![Edit::Set { row: RowRef::Existing(row_key), col, text: edit.text.clone() }];
                out.gesture = Some(Gesture::Commit);
                out.by_key = by_key;
                state.edit = None;
            }
            Err(message) => {
                out.refused = Some(message.clone());
                if by_key {
                    // The box gave up the keyboard on Enter: it takes it back, its text kept.
                    if let Some(open) = state.edit.as_mut() {
                        open.message = Some(message);
                        open.caret = Some(Caret::End);
                    }
                    return;
                }
                state.refused.push((row_key, col, message));
                state.edit = None;
                return;
            }
        }
    }
    if let Some(key) = key {
        state.focus = true;
        let (dr, dc): (isize, isize) = match key {
            crate::CellKey::Down => (1, 0),
            crate::CellKey::Up => (-1, 0),
            crate::CellKey::Next => (0, 1),
            crate::CellKey::Previous => (0, -1),
        };
        state.pending_move = Some((dr, dc, matches!(key, crate::CellKey::Next | crate::CellKey::Previous)));
    }
}

/// After drawing: moves a commit asked for, pointer presses and drags, and new edits.
fn pointer<S: GridSource>(
    ui: &Ui,
    state: &mut State,
    source: &mut S,
    shown: &Shown,
    columns: &[GridColumn],
    cells: &[(usize, usize, egui::Rect)],
    clicks: Vec<Click>,
    out: &mut GridOutput,
) {
    if let Some((dr, dc, wrap)) = state.pending_move.take() {
        step(state, source, shown, columns.len(), dr, dc, wrap, false);
    }
    if let Some(text) = state.pending_all.take()
        && let Some(range) = range(state, shown, columns.len())
    {
        state.refused.clear();
        let mut edits = Vec::new();
        for row in range.rows.0..=range.rows.1 {
            for col in range.cols.0..=range.cols.1 {
                write(state, source, shown, columns, row, col, &text, false, &mut edits, out);
            }
        }
        out.edits = edits;
    }
    // A press on another cell leaves the open edit: it commits first, as leaving the box does.
    if clicks.iter().any(|c| match *c {
        Click::Press { row, col, .. } | Click::Edit { row, col, .. } => state.edit.as_ref().is_some_and(|e| e.at.row != row || e.at.col != col),
    }) {
        commit(state, source, shown, None, false, out);
    }
    for click in clicks {
        match click {
            Click::Press { row, col, shift } => {
                let Some(key) = shown.source(row).map(|r| source.row_key(r).into_owned()) else { continue };
                let spot = Spot { key, row, col };
                if !(shift && state.cursor.is_some()) {
                    state.anchor = Some(spot.clone());
                }
                state.cursor = Some(spot);
                state.dragging = true;
                state.focus = true;
            }
            Click::Edit { row, col, caret } => {
                let Some(source_row) = shown.source(row) else { continue };
                if !kind_of(source, columns, source_row, col).types() {
                    continue;
                }
                let key = source.row_key(source_row).into_owned();
                let text = source.text(source_row, col).into_owned();
                let spot = Spot { key, row, col };
                state.cursor = Some(spot.clone());
                state.anchor = Some(spot.clone());
                state.edit = Some(Editing { at: spot, text, caret: Some(caret), unfocused: 0, had_focus: false, message: None });
                state.focus = false;
                state.dragging = false;
            }
        }
    }
    if state.dragging {
        let (down, at) = ui.input(|i| (i.pointer.primary_down(), i.pointer.interact_pos()));
        if !down {
            state.dragging = false;
        } else if let Some(at) = at
            && let Some((row, col, _)) = cells.iter().find(|(_, _, rect)| rect.contains(at))
            && state.cursor.as_ref().is_some_and(|c| c.row != *row || c.col != *col)
            && let Some(key) = shown.source(*row).map(|r| source.row_key(r).into_owned())
        {
            state.cursor = Some(Spot { key, row: *row, col: *col });
        }
    }
}

/// Write `text` into the shown cell (`row`, `col`) for a gesture, when its kind takes writes and
/// the source accepts it; otherwise note why on the cell.
fn write<S: GridSource>(
    state: &mut State,
    source: &mut S,
    shown: &Shown,
    columns: &[GridColumn],
    row: usize,
    col: usize,
    text: &str,
    clear: bool,
    edits: &mut Vec<Edit>,
    out: &mut GridOutput,
) {
    let Some(source_row) = shown.source(row) else { return };
    if col >= columns.len() || !kind_of(source, columns, source_row, col).writable() {
        return;
    }
    if !clear && source.text(source_row, col) == text {
        return;
    }
    let key = source.row_key(source_row).into_owned();
    match source.set_cell(source_row, col, text) {
        Ok(()) => {
            state.refused.retain(|(k, c, _)| !(*k == key && *c == col));
            edits.push(if clear {
                Edit::Clear { row: RowRef::Existing(key), col }
            } else {
                Edit::Set { row: RowRef::Existing(key), col, text: text.to_owned() }
            });
        }
        Err(message) => {
            out.refused.get_or_insert_with(|| message.clone());
            state.refused.push((key, col, message));
        }
    }
}

/// Move the cursor by `dr` rows and `dc` columns; with `wrap`, a column step past a row's end
/// goes on to the next row. `extend` keeps the anchor, growing the range.
fn step<S: GridSource>(state: &mut State, source: &S, shown: &Shown, cols: usize, dr: isize, dc: isize, wrap: bool, extend: bool) {
    let rows = shown.len();
    if rows == 0 || cols == 0 {
        return;
    }
    let (row, col) = state.cursor.as_ref().map_or((0, 0), |c| (c.row, c.col));
    let (mut row, mut col) = (row as isize + dr, col as isize + dc);
    if wrap {
        if col >= cols as isize && row + 1 < rows as isize {
            col = 0;
            row += 1;
        } else if col < 0 && row > 0 {
            col = cols as isize - 1;
            row -= 1;
        }
    }
    let row = row.clamp(0, rows as isize - 1) as usize;
    let col = col.clamp(0, cols as isize - 1) as usize;
    let key = shown.source(row).map(|r| source.row_key(r).into_owned()).unwrap_or_default();
    let spot = Spot { key, row, col };
    if !extend || state.anchor.is_none() {
        state.anchor = Some(spot.clone());
    }
    state.cursor = Some(spot);
    state.reveal = true;
}

/// A key pressed this frame with exactly these modifiers. `raw` reads the raw events (Enter and
/// Tab, as the table cells read them even when a dialog has consumed them); `repeat` takes held keys.
fn pressed(ui: &Ui, key: Key, shift: bool, command: bool, raw: bool, repeat: bool) -> bool {
    ui.input_mut(|i| {
        let events = if raw { &i.raw.events } else { &i.events };
        let hit = events.iter().any(|e| {
            matches!(e, Event::Key { key: k, pressed: true, repeat: r, modifiers, .. }
                if *k == key && (repeat || !*r) && modifiers.shift == shift && modifiers.command == command && !modifiers.alt)
        });
        if hit {
            i.events.retain(|e| !matches!(e, Event::Key { key: k, pressed: true, .. } if *k == key));
        }
        hit
    })
}

/// Keys while the grid has the keyboard and no cell is edited.
fn navigate<S: GridSource>(ui: &Ui, state: &mut State, source: &mut S, shown: &Shown, columns: &[GridColumn], out: &mut GridOutput) {
    let cols = columns.len();
    let rows = shown.len();
    if rows == 0 || cols == 0 {
        return;
    }
    if state.cursor.is_none() {
        step(state, source, shown, cols, 0, 0, false, false);
    }
    for shift in [false, true] {
        for (key, dr, dc) in [(Key::ArrowDown, 1, 0), (Key::ArrowUp, -1, 0), (Key::ArrowRight, 0, 1), (Key::ArrowLeft, 0, -1)] {
            if pressed(ui, key, shift, false, false, true) {
                step(state, source, shown, cols, dr, dc, false, shift);
            }
            if pressed(ui, key, shift, true, false, true) {
                let far = (rows.max(cols)) as isize;
                step(state, source, shown, cols, dr * far, dc * far, false, shift);
            }
        }
        for (key, command, dr, dc) in [
            (Key::Home, false, 0, -(cols as isize)),
            (Key::End, false, 0, cols as isize),
            (Key::Home, true, -(rows as isize), -(cols as isize)),
            (Key::End, true, rows as isize, cols as isize),
        ] {
            if pressed(ui, key, shift, command, false, false) {
                step(state, source, shown, cols, dr, dc, false, shift);
            }
        }
    }
    for (key, shift, dr, dc, wrap) in
        [(Key::Tab, false, 0, 1, true), (Key::Tab, true, 0, -1, true), (Key::Enter, false, 1, 0, false), (Key::Enter, true, -1, 0, false)]
    {
        if pressed(ui, key, shift, false, true, false) {
            step(state, source, shown, cols, dr, dc, wrap, false);
        }
    }
    if pressed(ui, Key::A, false, true, false, false) {
        let last = shown.source(rows - 1).map(|r| source.row_key(r).into_owned()).unwrap_or_default();
        let first = shown.source(0).map(|r| source.row_key(r).into_owned()).unwrap_or_default();
        state.anchor = Some(Spot { key: first, row: 0, col: 0 });
        state.cursor = Some(Spot { key: last, row: rows - 1, col: cols - 1 });
    }
    let Some(range) = range(state, shown, cols) else { return };
    let cursor = state.cursor.clone().unwrap_or_default();
    let cursor_row = shown.source(cursor.row);
    let cursor_kind = cursor_row.map_or(CellKind::ReadOnly, |r| kind_of(source, columns, r, cursor.col));
    if !range.single() && pressed(ui, Key::Escape, false, false, false, false) {
        state.anchor = Some(cursor.clone());
        return;
    }
    let (copy, cut, paste) = ui.input(|i| {
        let copy = i.events.iter().any(|e| matches!(e, Event::Copy));
        let cut = i.events.iter().any(|e| matches!(e, Event::Cut));
        let paste = i.events.iter().find_map(|e| match e {
            Event::Paste(text) => Some(text.clone()),
            _ => None,
        });
        (copy, cut, paste)
    });
    let copy = copy | pressed(ui, Key::C, false, true, false, false);
    let cut = cut | pressed(ui, Key::X, false, true, false, false);
    if copy || cut {
        let block: Vec<Vec<String>> = (range.rows.0..=range.rows.1)
            .map(|row| {
                (range.cols.0..=range.cols.1).map(|col| shown.source(row).map(|r| source.text(r, col).into_owned()).unwrap_or_default()).collect()
            })
            .collect();
        ui.ctx().copy_text(write_tsv(&block));
        ui.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Copy | Event::Cut)));
    }
    if let Some(text) = paste {
        ui.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Paste(_))));
        paste_block(state, source, shown, columns, range, &text, out);
        return;
    }
    if cut || pressed(ui, Key::Delete, false, false, false, false) || pressed(ui, Key::Backspace, false, false, false, false) {
        state.refused.clear();
        let mut edits = Vec::new();
        for row in range.rows.0..=range.rows.1 {
            for col in range.cols.0..=range.cols.1 {
                write(state, source, shown, columns, row, col, "", true, &mut edits, out);
            }
        }
        if !edits.is_empty() {
            out.edits = edits;
            out.gesture = Some(Gesture::Clear);
        }
        return;
    }
    if pressed(ui, Key::D, false, true, false, false) {
        // The top row of the range down the rest of it; a one-row range takes the row above.
        let (from, to) = if range.rows.0 == range.rows.1 {
            (range.rows.0.checked_sub(1), range.rows.0..=range.rows.1)
        } else {
            (Some(range.rows.0), range.rows.0 + 1..=range.rows.1)
        };
        let Some(from) = from.and_then(|row| shown.source(row)) else { return };
        state.refused.clear();
        let mut edits = Vec::new();
        for col in range.cols.0..=range.cols.1 {
            let text = source.text(from, col).into_owned();
            for row in to.clone() {
                write(state, source, shown, columns, row, col, &text, false, &mut edits, out);
            }
        }
        if !edits.is_empty() {
            out.edits = edits;
            out.gesture = Some(Gesture::Fill);
        }
        return;
    }
    if matches!(cursor_kind, CellKind::Check) && pressed(ui, Key::Space, false, false, false, false) {
        if let Some(row) = cursor_row {
            let next = if checked(&source.text(row, cursor.col)) { "false" } else { "true" };
            let mut edits = Vec::new();
            write(state, source, shown, columns, cursor.row, cursor.col, next, false, &mut edits, out);
            if !edits.is_empty() {
                out.edits = edits;
                out.gesture = Some(Gesture::Commit);
            }
        }
        return;
    }
    if !cursor_kind.types() {
        return;
    }
    // The range stays: Ctrl+Enter writes the edit to all of it.
    let start = |state: &mut State, text: String, caret: Caret| {
        state.edit = Some(Editing { at: cursor.clone(), text, caret: Some(caret), unfocused: 0, had_focus: false, message: None });
    };
    if pressed(ui, Key::F2, false, false, false, false) {
        let text = cursor_row.map(|r| source.text(r, cursor.col).into_owned()).unwrap_or_default();
        start(state, text, Caret::All);
        return;
    }
    // Typing replaces the cell: the first character opens the editor with it. An IME composition
    // opens an empty editor and goes on into it.
    let typed = ui.input(|i| {
        i.events.iter().find_map(|e| match e {
            Event::Text(text) if !text.is_empty() && !text.chars().any(char::is_control) && !i.modifiers.command => Some(text.clone()),
            _ => None,
        })
    });
    if let Some(text) = typed {
        ui.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Text(_))));
        start(state, text, Caret::End);
        return;
    }
    if ui.input(|i| i.events.iter().any(|e| matches!(e, Event::Ime(egui::ImeEvent::Preedit { text, .. }) if !text.is_empty()))) {
        start(state, String::new(), Caret::End);
    }
}

/// Paste tab-separated `text` at the range: one value fills the whole range; a block goes from
/// the range's top-left cell, adding rows below the last when the source can, else cut off with
/// a notice. Read-only cells in its way keep their text.
fn paste_block<S: GridSource>(
    state: &mut State,
    source: &mut S,
    shown: &Shown,
    columns: &[GridColumn],
    range: Range,
    text: &str,
    out: &mut GridOutput,
) {
    state.refused.clear();
    let block = read_tsv(text);
    let mut edits = Vec::new();
    if let [row] = block.as_slice()
        && let [value] = row.as_slice()
    {
        for row in range.rows.0..=range.rows.1 {
            for col in range.cols.0..=range.cols.1 {
                write(state, source, shown, columns, row, col, value, false, &mut edits, out);
            }
        }
    } else {
        let rows = shown.len();
        let grows = source.add_row().is_some();
        for (offset, values) in block.iter().enumerate() {
            let row = range.rows.0 + offset;
            for (dc, value) in values.iter().enumerate() {
                let col = range.cols.0 + dc;
                if col >= columns.len() {
                    break;
                }
                if row < rows {
                    write(state, source, shown, columns, row, col, value, false, &mut edits, out);
                } else if grows && columns.get(col).is_some_and(|c| c.kind.writable()) {
                    edits.push(Edit::Set { row: RowRef::New(row - rows), col, text: value.clone() });
                }
            }
            if row >= rows && !grows {
                let fitted = rows.saturating_sub(range.rows.0);
                out.notice =
                    Some(solidcraft_ui_text::format("grid.paste_cut_off", &[("rows", &fitted.to_string()), ("total", &block.len().to_string())]));
                break;
            }
        }
        // The pasted block becomes the range.
        let last_row = (range.rows.0 + block.len().saturating_sub(1)).min(rows.saturating_sub(1));
        let width = block.iter().map(Vec::len).max().unwrap_or(1).max(1);
        let last_col = (range.cols.0 + width - 1).min(columns.len().saturating_sub(1));
        if let Some(key) = shown.source(last_row).map(|r| source.row_key(r).into_owned()) {
            state.cursor = Some(Spot { key, row: last_row, col: last_col });
        }
        if let Some(key) = shown.source(range.rows.0).map(|r| source.row_key(r).into_owned()) {
            state.anchor = Some(Spot { key, row: range.rows.0, col: range.cols.0 });
        }
    }
    if !edits.is_empty() {
        out.edits = edits;
        out.gesture = Some(Gesture::Paste);
    }
}

#[cfg(test)]
#[path = "grid_tests.rs"]
mod tests;
