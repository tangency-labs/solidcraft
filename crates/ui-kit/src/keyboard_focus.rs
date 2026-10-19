//! Keyboard focus across the app. Not a drawn element, so it lives outside the element files the
//! gallery guard checks.
//!
//! - Tab or Shift+Tab into a text or number box selects its text, so typing replaces the value. A
//!   click still puts the caret where it lands. One egui plugin does this for every kit text box
//!   ([`install_keyboard_focus`]); egui's own `DragValue` already selects on focus.
//! - Tables move the keyboard between cells: Enter commits a cell and goes to the one below in the
//!   same column, Shift+Enter to the one above ([`cell_key`], [`land_in_cell`], [`landing`]).
use egui::text::{CCursor, CCursorRange};
use std::collections::HashMap;

/// Select every character of the box `id` shows `text` in, with the caret at the end.
pub fn select_all_text(ctx: &egui::Context, id: egui::Id, text: &str) {
    let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(text.chars().count()))));
    state.store(ctx, id);
}

/// Make Tab and Shift+Tab select the text of the box they land in, in every viewport of `ctx`.
/// Cheap to call every frame: the plugin is added once.
pub fn install_keyboard_focus(ctx: &egui::Context) {
    let installed = egui::Id::unique("sc_keyboard_focus_installed");
    if !ctx.data(|d| d.get_temp::<bool>(installed).unwrap_or(false)) {
        ctx.add_plugin(TabSelects::default());
        ctx.data_mut(|d| d.insert_temp(installed, true));
    }
}

fn focused_text_id() -> egui::Id {
    egui::Id::unique("sc_keyboard_focus_text")
}

/// A kit text box with the keyboard notes its length each pass, so the plugin selects exactly its
/// characters when the keyboard arrived by Tab. Called by `text_menu::menu`, which every kit text
/// box draws through.
pub(crate) fn note_focused_text(ctx: &egui::Context, id: egui::Id, chars: usize) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(focused_text_id(), (pass, id, 0..chars)));
}

/// A host expression field may select only its numeric character range on Tab.
pub(crate) fn note_focused_selection(ctx: &egui::Context, id: egui::Id, range: std::ops::Range<usize>) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(focused_text_id(), (pass, id, range)));
}

/// Per viewport: the box that had the keyboard at the end of the last pass, and the frame Tab was
/// last pressed in.
#[derive(Default)]
struct TabSelects {
    tab_pressed: bool,
    tab_frame: HashMap<egui::ViewportId, u64>,
    last: HashMap<egui::ViewportId, Option<egui::Id>>,
}

impl egui::Plugin for TabSelects {
    fn debug_name(&self) -> &'static str {
        "solidcraft_tab_selects"
    }

    // Raw input, before any widget consumes the key.
    fn input_hook(&mut self, _ctx: &egui::Context, input: &mut egui::RawInput) {
        self.tab_pressed |= input.events.iter().any(|event| {
            matches!(event, egui::Event::Key { key: egui::Key::Tab, pressed: true, modifiers, .. }
                if !modifiers.any() || modifiers.shift_only())
        });
    }

    fn on_end_pass(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let viewport = ctx.viewport_id();
        let frame = ctx.cumulative_frame_nr();
        if std::mem::take(&mut self.tab_pressed) {
            self.tab_frame.insert(viewport, frame);
        }
        let focused = ctx.memory(|m| m.focused());
        let before = self.last.insert(viewport, focused).flatten();
        let Some(id) = focused.filter(|id| Some(*id) != before) else { return };
        // Shift+Tab hands the focus over a frame later than Tab does.
        let by_tab = self.tab_frame.get(&viewport).is_some_and(|tab| frame <= tab + 1);
        let by_pointer = ctx.input(|i| i.pointer.any_pressed() || i.pointer.any_down() || i.pointer.any_released());
        if !by_tab || by_pointer {
            return;
        }
        let pass = ctx.cumulative_pass_nr();
        let noted = ctx.data(|d| d.get_temp::<(u64, egui::Id, std::ops::Range<usize>)>(focused_text_id()));
        if let Some((_, _, range)) = noted.filter(|(drawn, noted, _)| *drawn == pass && *noted == id) {
            let mut state = egui::text_edit::TextEditState::load(&ctx, id).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(range.start), CCursor::new(range.end))));
            state.store(&ctx, id);
        }
    }
}

/// A keyboard move out of a table cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellKey {
    /// Enter: commit, then the cell below in the same column.
    Down,
    /// Shift+Enter: commit, then the cell above.
    Up,
    /// Tab: the next cell of the row, then the first of the next row.
    Next,
    /// Shift+Tab: the previous cell, then the last of the row above.
    Previous,
}

/// The fresh Enter, Shift+Enter, Tab or Shift+Tab pressed this pass, read from the raw events as
/// the cells' own commits read them (an accepted completion has already taken its Enter). Only
/// `focused` cells ask: the one that has the keyboard or had it last pass.
pub fn cell_key(ui: &egui::Ui, focused: bool) -> Option<CellKey> {
    if !focused {
        return None;
    }
    ui.input(|i| {
        i.raw.events.iter().find_map(|event| match event {
            egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } => {
                let shift = modifiers.shift_only();
                if !shift && modifiers.any() {
                    return None;
                }
                match key {
                    egui::Key::Enter => Some(if shift { CellKey::Up } else { CellKey::Down }),
                    egui::Key::Tab => Some(if shift { CellKey::Previous } else { CellKey::Next }),
                    _ => None,
                }
            }
            _ => None,
        })
    })
}

/// Where a keyboard move in a table lands: its row and column keys.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Landing {
    row: String,
    column: String,
}

fn landing_id(table: egui::Id) -> egui::Id {
    table.with("sc_cell_landing")
}

/// Send the keyboard of `table` to the cell at `row`, `column`. It takes the focus, with its text
/// selected, when it draws next ([`landing`]).
pub fn land_in_cell(ctx: &egui::Context, table: egui::Id, row: &str, column: &str) {
    ctx.data_mut(|d| d.insert_temp(landing_id(table), Landing { row: row.to_owned(), column: column.to_owned() }));
}

/// Whether a keyboard move of `table` is about to land in `row`. A table builds that row even off
/// screen, so the cell can take the focus and scroll into view.
pub fn landing_in_row(ctx: &egui::Context, table: egui::Id, row: &str) -> bool {
    ctx.data(|d| d.get_temp::<Landing>(landing_id(table))).is_some_and(|l| l.row == row)
}

/// Called by each cell after drawing its box: when this cell is where a keyboard move lands, it
/// takes the focus with `text` selected and scrolls into view. Returns whether it landed.
pub fn landing(ui: &egui::Ui, table: egui::Id, row: &str, column: &str, response: &egui::Response, text: &str) -> bool {
    let id = landing_id(table);
    let here = ui.ctx().data(|d| d.get_temp::<Landing>(id)).is_some_and(|l| l.row == row && l.column == column);
    if here {
        ui.ctx().data_mut(|d| d.remove_temp::<Landing>(id));
        response.request_focus();
        select_all_text(ui.ctx(), response.id, text);
        response.scroll_to_me(None);
    }
    here
}

/// Keep Tab and Shift+Tab with the focused cell `response` for the next pass, so the table moves
/// between its cells instead of egui's focus order stepping onto the row's buttons. Arrows stay
/// with the caret.
pub fn keep_tab_in_cell(ui: &egui::Ui, response: &egui::Response) {
    if response.has_focus() {
        let filter = egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: false };
        ui.memory_mut(|m| m.set_focus_lock_filter(response.id, filter));
    }
}

#[cfg(test)]
#[path = "keyboard_focus_tests.rs"]
mod tests;
