//! Up/Down and the mouse wheel step the number in a focused value field (#2320): one per press
//! or wheel notch, ten with Shift, a tenth with Alt. Only a plain number steps, alone or with a
//! unit after it ("20", "20 mm", "2,5 mm", "45 deg", `3"`), and it keeps the unit and the decimal
//! mark it is written with; anything else (an expression such as `width / 2`, an empty field) is
//! left as typed, and Up/Down move its text cursor as before. Every expression field gets this
//! through `params_dialog::complete`, after its completions: while the completion list is open it
//! keeps the arrows (`expression_complete::begin_frame` takes them before the field sees them).
//! It is a preference (Preferences › Design, `step_values`), off by default: off, the fields
//! behave as before.

use egui::{Event, Key, Modifiers, MouseWheelUnit};

fn enabled_id() -> egui::Id {
    egui::Id::unique("value_step_enabled")
}

/// Turn stepping on or off for this frame, from the preference (the app calls this every frame).
pub(crate) fn set_enabled(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(enabled_id(), on));
}

/// Whether value fields step this frame.
pub(crate) fn enabled(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(enabled_id())).unwrap_or(false)
}

/// Most wheel notches one frame may add, so a runaway or hostile delta can't fling the value.
const MAX_NOTCHES: f32 = 100.0;

/// Step the number in `text` for this frame's Up/Down presses and, with the pointer over the
/// field, its wheel input, while `resp`'s field has the keyboard. The wheel over such a field
/// steps it instead of scrolling the panel under it. Returns whether the text changed; the whole
/// new text is then selected, so typing replaces it, as after a pick.
pub(crate) fn step(ui: &egui::Ui, resp: &egui::Response, text: &mut String) -> bool {
    let wheel_key = resp.id.with("value_step_wheel");
    let hovered = resp.hovered();
    if !hovered {
        ui.data_mut(|d| d.remove::<f32>(wheel_key));
    }
    if !enabled(ui.ctx()) || !resp.has_focus() || plain(text).is_none() {
        return false;
    }
    let mut tenths = ui.input(key_tenths);
    if hovered {
        tenths = tenths.saturating_add(wheel_tenths(ui, wheel_key));
        crate::navigation::claim_wheel(ui.ctx());
    }
    if tenths == 0 {
        return false;
    }
    let Some(new) = stepped(text, tenths) else { return false };
    // Nothing after the field acts on these keys as well.
    ui.ctx().input_mut(|i| {
        i.events.retain(
            |e| !matches!(e, Event::Key { key: Key::ArrowUp | Key::ArrowDown, pressed: true, modifiers, .. } if tenths_for(*modifiers).is_some()),
        );
    });
    *text = new;
    let n = text.chars().count();
    let mut state = egui::TextEdit::load_state(ui.ctx(), resp.id).unwrap_or_default();
    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(n))));
    state.store(ui.ctx(), resp.id);
    // The field was drawn with the old text this frame.
    ui.ctx().request_repaint();
    true
}

/// One step in tenths for these modifiers: 10 (one), 100 with Shift, 1 with Alt. `None` with Ctrl
/// or Cmd held, which keep their own meaning (the text cursor's Up/Down, the wheel's zoom).
fn tenths_for(m: Modifiers) -> Option<i64> {
    if m.ctrl || m.command || m.mac_cmd {
        return None;
    }
    match (m.shift, m.alt) {
        (false, false) => Some(10),
        (true, false) => Some(100),
        (false, true) => Some(1),
        (true, true) => None,
    }
}

/// This frame's Up/Down presses, key repeats included, in signed tenths.
fn key_tenths(i: &egui::InputState) -> i64 {
    i.events
        .iter()
        .filter_map(|e| match e {
            Event::Key { key: Key::ArrowUp, pressed: true, modifiers, .. } => tenths_for(*modifiers),
            Event::Key { key: Key::ArrowDown, pressed: true, modifiers, .. } => tenths_for(*modifiers).map(|t| -t),
            _ => None,
        })
        .fold(0, i64::saturating_add)
}

/// This frame's wheel notches over the field in signed tenths. Scrolling up steps up. A mouse
/// wheel reports lines (a notch each); a trackpad reports points, which add up across frames in
/// `key` until they make a notch (a line's worth, `line_scroll_speed`).
fn wheel_tenths(ui: &egui::Ui, key: egui::Id) -> i64 {
    let speed = ui.ctx().options(|o| o.input_options.line_scroll_speed).max(1.0);
    let mut pending = ui.data(|d| d.get_temp::<f32>(key)).unwrap_or(0.0);
    let mut out = 0i64;
    ui.input(|i| {
        for e in &i.events {
            let Event::MouseWheel { unit, delta, modifiers, .. } = e else { continue };
            let Some(t) = tenths_for(*modifiers) else { continue };
            // macOS turns Shift+wheel into sideways scrolling.
            let d = if delta.y == 0.0 && modifiers.shift { delta.x } else { delta.y };
            let notches = match unit {
                MouseWheelUnit::Point => d / speed,
                MouseWheelUnit::Line | MouseWheelUnit::Page => d,
            };
            if !notches.is_finite() {
                continue;
            }
            pending = (pending + notches).clamp(-MAX_NOTCHES, MAX_NOTCHES);
            let whole = pending.trunc();
            pending -= whole;
            out = out.saturating_add((whole as i64).saturating_mul(t));
        }
    });
    ui.data_mut(|d| d.insert_temp(key, pending));
    out
}

/// A plain number in a field's text: where the number is, its value, its decimal places and
/// decimal mark.
struct Plain {
    start: usize,
    end: usize,
    value: f64,
    places: usize,
    comma: bool,
}

/// `text` as a plain number: an optional sign, digits with at most one decimal mark (a point, or
/// a comma as expressions read it: `2,5 mm`), and at most a unit after it (one `expr::unit_info`
/// knows, or an inch or foot mark); `None` for anything else.
fn plain(text: &str) -> Option<Plain> {
    let start = text.len() - text.trim_start().len();
    let body = text.get(start..)?;
    let (mut len, mut digits, mut places, mut mark) = (0, 0, 0, None);
    for (i, c) in body.char_indices() {
        match c {
            '+' | '-' if i == 0 => {}
            '0'..='9' => {
                digits += 1;
                places += usize::from(mark.is_some());
            }
            '.' if mark.is_none() => mark = Some(c),
            ',' if mark.is_none() && digits > 0 => mark = Some(c),
            _ => break,
        }
        len = i + c.len_utf8();
    }
    let unit = body.get(len..)?.trim();
    let unit_ok = unit.is_empty() || matches!(unit, "\"" | "″" | "'" | "′") || solidcraft_engine::doc::expr::unit_info(unit).is_some();
    // A comma reads as a decimal mark only between digits.
    if digits == 0 || !unit_ok || (mark == Some(',') && places == 0) {
        return None;
    }
    let value = body.get(..len)?.replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite())?;
    Some(Plain { start, end: start + len, value, places, comma: mark == Some(',') })
}

/// `text` with its plain number moved by `tenths` tenths, in the places it was written with (one
/// more for a tenth step on a whole number): "20 mm" up one is "21 mm", "2.5" up a tenth "2.6".
/// `None` when `text` is not a plain number.
fn stepped(text: &str, tenths: i64) -> Option<String> {
    let p = plain(text)?;
    let places = if tenths % 10 == 0 { p.places } else { p.places.max(1) };
    let v = p.value + tenths as f64 / 10.0;
    if !v.is_finite() {
        return None;
    }
    let mut number = format!("{v:.places$}");
    // A step to zero from below reads "0", not "-0".
    if let Some(magnitude) = number.strip_prefix('-')
        && magnitude.chars().all(|c| c == '0' || c == '.')
    {
        number = magnitude.to_string();
    }
    if p.comma {
        number = number.replace('.', ",");
    }
    Some(format!("{}{number}{}", text.get(..p.start)?, text.get(p.end..)?))
}

#[cfg(test)]
#[path = "value_step_tests.rs"]
mod tests;
