//! Numeric editing and keyboard focus bookkeeping. Not a drawn element, so it
//! lives outside the element files the gallery guard checks.

/// A short pause disambiguates a unit such as `m` while `mm` is being typed.
const UNIT_PREFIX_PAUSE: std::time::Duration = std::time::Duration::from_millis(300);

#[derive(Clone)]
struct NumericEdit {
    id: egui::Id,
    text: String,
    edited_at: f64,
    pass: u64,
}

fn numeric_edit_id() -> egui::Id {
    egui::Id::unique("sc_numeric_edit")
}

/// Record a numeric editor's text without changing its editing or completion contract.
/// Native number controls supply None on frames when their parser was not called.
/// `edited` includes typing that did not parse to a changed numeric value.
/// Completion or stepping sets `committed` to accept an explicitly chosen value immediately.
/// Only an unfinished unit delays the preview; complete values remain immediate.
pub fn note_numeric_edit(ui: &egui::Ui, response: &egui::Response, text: Option<&str>, edited: bool, committed: bool) {
    let ctx = ui.ctx();
    let previous = ctx.data(|data| data.get_temp::<NumericEdit>(numeric_edit_id()));
    if committed || !response.has_focus() || response.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        if previous.as_ref().is_some_and(|edit| edit.id == response.id) {
            ctx.data_mut(|data| data.remove::<NumericEdit>(numeric_edit_id()));
        }
        return;
    }
    let previous = previous.filter(|edit| edit.id == response.id);
    let Some(text) = text.or_else(|| previous.as_ref().map(|edit| edit.text.as_str())) else { return };
    if !crate::expressions(ctx).is_some_and(|host| pending_unit(host.as_ref(), text)) {
        ctx.data_mut(|data| data.remove::<NumericEdit>(numeric_edit_id()));
        return;
    }
    let edited_at = if edited {
        ui.input(|i| i.time)
    } else if let Some(edit) = previous.as_ref().filter(|edit| edit.text == text) {
        edit.edited_at
    } else {
        // Focusing an existing complete value, or changing it outside text editing, keeps
        // the existing immediate preview behaviour.
        ctx.data_mut(|data| data.remove::<NumericEdit>(numeric_edit_id()));
        return;
    };
    let edit = NumericEdit { id: response.id, text: text.to_owned(), edited_at, pass: ctx.cumulative_pass_nr() };
    ctx.data_mut(|data| data.insert_temp(numeric_edit_id(), edit));
}

/// Whether live previews may consume numeric fields this pass. Enter and focus loss commit
/// immediately. A pending prefix requests a repaint so a quiet editor publishes after the pause.
pub fn numeric_preview_ready(ctx: &egui::Context) -> bool {
    let Some(edit) = ctx.data(|data| data.get_temp::<NumericEdit>(numeric_edit_id())) else { return true };
    // The viewport schedules its preview before the dialog draws this pass, so it consumes
    // the previous pass's fields. Also allow a host that schedules after drawing its fields.
    if ctx.cumulative_pass_nr().saturating_sub(edit.pass) > 1 || !ctx.memory(|m| m.has_focus(edit.id)) {
        return true;
    }
    // A queued edit must reach the field before an expired timer, or an Enter in the same
    // frame, can publish its old text (`m` then Enter must not release the earlier `2m`).
    // This matters when the host schedules previews before processing the current inputs.
    if ctx.input(|i| {
        i.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Text(_)
                    | egui::Event::Paste(_)
                    | egui::Event::Cut
                    | egui::Event::Key { key: egui::Key::Backspace | egui::Key::Delete, pressed: true, .. }
            )
        })
    }) {
        ctx.request_repaint();
        return false;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
        return true;
    }
    let remaining = edit.edited_at + UNIT_PREFIX_PAUSE.as_secs_f64() - ctx.input(|i| i.time);
    if remaining > 0.0
        && let Ok(delay) = std::time::Duration::try_from_secs_f64(remaining)
    {
        ctx.request_repaint_after(delay);
        false
    } else {
        true
    }
}

pub(super) fn pending_unit(host: &dyn crate::Expressions, text: &str) -> bool {
    // A separator terminates the unit. Keep whole identifiers together so a parameter such
    // as `width2m` never looks like a number followed by `m`.
    let start = text.char_indices().rev().find(|(_, c)| !(c.is_alphanumeric() || *c == '_')).map_or(0, |(at, c)| at + c.len_utf8());
    let word = text.get(start..).unwrap_or_default();
    let unit_start = word.char_indices().rev().find(|(_, c)| !c.is_alphabetic()).map_or(0, |(at, c)| at + c.len_utf8());
    let unit = word.get(unit_start..).unwrap_or_default();
    let number = word.get(..unit_start).unwrap_or_default();
    if unit.is_empty() || !host.unit_is_prefix(unit) {
        return false;
    }
    if !number.is_empty() {
        return crate::parse_number(number).is_some();
    }
    host.is_unit(unit) || text.get(..start).unwrap_or_default().trim_end().chars().next_back().is_some_and(|c| c.is_ascii_digit() || c == ')')
}

/// Collect the expression editors drawn by a form, in display order.
pub fn value_fields(ctx: &egui::Context, draw: impl FnOnce() -> bool) -> (bool, Vec<(egui::Id, usize)>) {
    let id = egui::Id::unique("sc_value_fields");
    ctx.data_mut(|data| data.insert_temp(id, Vec::<(egui::Id, usize)>::new()));
    let enter = draw();
    let fields = ctx.data_mut(|data| data.remove_temp::<Vec<(egui::Id, usize)>>(id)).unwrap_or_default();
    (enter, fields)
}

/// The expression editor the form drew last so far, in [`value_fields`].
pub fn last_value_field(ctx: &egui::Context) -> Option<egui::Id> {
    ctx.data(|data| data.get_temp::<Vec<(egui::Id, usize)>>(egui::Id::unique("sc_value_fields"))).and_then(|fields| fields.last().map(|(id, _)| *id))
}

/// True while a text editor drawn by the kit holds the keyboard. A dialog uses this to keep at
/// most one field looking active: its pick fields drop their accent ring while a text field
/// has the keyboard (#10066).
pub fn editor_has_focus(ctx: &egui::Context) -> bool {
    ctx.data(|data| data.get_temp::<egui::Id>(egui::Id::unique("sc_focused_editor")))
        .is_some_and(|id| ctx.memory(|memory| memory.focused() == Some(id)))
}

/// Only a value box drawn this pass can receive the keyboard after a viewport pick.
pub fn canvas_value_focus(ctx: &egui::Context) -> Option<(egui::Id, usize)> {
    ctx.data(|data| data.get_temp::<(u64, egui::Id, usize)>(egui::Id::unique("sc_canvas_value_focus")))
        .filter(|(pass, _, _)| *pass == ctx.cumulative_pass_nr())
        .map(|(_, id, length)| (id, length))
}
