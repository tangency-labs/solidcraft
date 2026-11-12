//! Renaming a tree row in the row itself, the same way in the browser and in the History on the
//! left. A slower second click on a row's name (the same row clicked again, plainly, after the
//! double-click delay) is held back for that delay and then renames, so the first click of a
//! double-click never starts a rename and a double-click keeps editing or opening the row. Both
//! trees call [`slow_click`] and [`editor`], so the gesture and the editor's look cannot drift
//! apart.
use egui::{Rect, pos2};

use crate::SolveApp;
use crate::context_menu::RenameWhat;
use solidcraft_theme::Tokens;

/// A second click on the same name later than this is a new gesture, not a rename.
const SLOW_RENAME_WINDOW: f64 = 3.0;

/// What a row does with this frame's click.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Click {
    /// The row acts on a click this frame (a held-back slow click comes back as one).
    pub clicked: bool,
    /// The click is a slow second click: rename the row.
    pub rename: bool,
}

/// With a dialog open a second click on a row picks again, so it never waits to rename.
pub(crate) fn dialog_open(ui: &egui::Ui) -> bool {
    ui.ctx().data(|d| d.get_temp::<bool>(egui::Id::unique("sc_tree_dialog_open"))).unwrap_or(false)
}

/// The click handling of the row `row`: `pressed` is whether its response was clicked this frame,
/// `clicked` whether the row takes that as a click (the browser's semantic routing may change
/// it), `double` a double-click, and `renames` whether the row has a name a slow click renames.
pub(crate) fn slow_click(ui: &egui::Ui, row: egui::Id, pressed: bool, clicked: bool, double: bool, renames: bool) -> Click {
    // Only a plain click: Ctrl and Shift keep adding to and extending the selection.
    let plain = !ui.input(|i| i.modifiers.command || i.modifiers.ctrl || i.modifiers.shift);
    let name_click = renames && clicked && !double && plain && !dialog_open(ui);
    let mut out = Click { clicked, rename: false };
    let last_key = egui::Id::unique("sc_browser_last_name_click");
    let pending_key = egui::Id::unique("sc_browser_pending_rename");
    let now = ui.input(|i| i.time);
    if !now.is_finite() {
        return out;
    }
    let delay = ui.ctx().options(|options| options.input_options.max_double_click_delay);
    let any_press = ui.input(|i| i.pointer.any_pressed());
    // (row, due, made)
    if let Some((pending_row, due, made)) = ui.ctx().data(|data| data.get_temp::<(egui::Id, f64, f64)>(pending_key)) {
        let stale = !(due.is_finite() && made.is_finite()) || now > due + SLOW_RENAME_WINDOW;
        if stale || (any_press && now > made) || (pending_row == row && double) {
            ui.ctx().data_mut(|data| data.remove::<(egui::Id, f64, f64)>(pending_key));
        } else if pending_row == row {
            if now >= due {
                ui.ctx().data_mut(|data| data.remove::<(egui::Id, f64, f64)>(pending_key));
                return Click { clicked: true, rename: true };
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64((due - now).clamp(0.0, SLOW_RENAME_WINDOW)));
        }
    }
    if !pressed {
        return out;
    }
    let previous = ui.ctx().data_mut(|data| {
        let previous = data.get_persisted::<(egui::Id, f64, bool)>(last_key);
        data.insert_persisted(last_key, (row, now, name_click));
        previous
    });
    let slow = name_click
        && previous.is_some_and(|(previous_row, previous_time, previous_was_name)| {
            previous_row == row
                && previous_was_name
                && previous_time.is_finite()
                && now - previous_time > delay
                && now - previous_time <= SLOW_RENAME_WINDOW
        });
    if slow {
        ui.ctx().data_mut(|data| data.insert_temp(pending_key, (row, now + delay, now)));
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(delay.clamp(0.0, SLOW_RENAME_WINDOW)));
        out.clicked = false;
    }
    out
}

/// Rename `what` in the row `row` instead of the floating box.
pub(crate) fn start(app: &mut SolveApp, what: RenameWhat, row: egui::Id, at: egui::Pos2) {
    crate::context_menu::start_inline_rename(app, what, at, row);
}

/// A rename of `what` opened without a row (a menu, F2, a new folder or automation) is shown in
/// the row `row` instead, when that row stands for it.
pub(crate) fn adopt(app: &mut SolveApp, what: &RenameWhat, row: egui::Id) {
    if let Some(rename) = app.menu.rename.as_mut().filter(|rename| !rename.inline && rename.what == *what) {
        rename.inline = true;
        rename.inline_id = Some(row);
    }
}

/// The name of the row `row` edited over `label` (where the name is drawn), when that is the
/// rename under way: Enter or leaving the box keeps the name, Escape cancels. `handle` names the
/// editor for automation. Returns the editor's rectangle while it is shown.
pub(crate) fn editor(app: &mut SolveApp, ui: &mut egui::Ui, row: egui::Id, label: Rect, handle: &str) -> Option<Rect> {
    if !app.menu.rename.as_ref().is_some_and(|rename| rename.inline && rename.inline_id == Some(row)) {
        return None;
    }
    let mut rename = app.menu.rename.take()?;
    let t = Tokens::get();
    // The field reaches back toward the icon; the text stays where the label was.
    let field = Rect::from_min_max(pos2(label.left() - t.space_3, label.top()), label.max);
    ui.painter().rect_filled(field, t.radius_sm, t.field);
    let id = ui.scope_id().with(("row_rename", row));
    let response = crate::ui_kit::inline_name(ui, label, id, &mut rename.text);
    crate::panel_runtime::register_content_focus(ui.ctx(), &response);
    crate::scenario::publish_handle(handle, label.center());
    let was_focused = rename.focused;
    if !rename.focused {
        // The whole name selected from the first frame (the editor may have no state yet), and
        // drawn at once rather than at the next pointer move.
        let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
        let end = rename.text.chars().count();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(end))));
        state.store(ui.ctx(), id);
        response.request_focus();
        ui.ctx().request_repaint();
        rename.focused = true;
    }
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let commit = ui.input(|i| i.key_pressed(egui::Key::Enter)) || (was_focused && response.lost_focus());
    if escape {
        // Dropping the state cancels the edit.
    } else if commit {
        if crate::context_menu::commit_rename(app, &rename).is_err() {
            app.menu.rename = Some(rename);
        }
    } else {
        app.menu.rename = Some(rename);
    }
    Some(label)
}
