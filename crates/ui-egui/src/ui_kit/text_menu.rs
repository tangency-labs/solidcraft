//! The right-click menu of the kit's text fields (#8875): Cut, Copy, Paste and Select All, the
//! edits Ctrl+X, Ctrl+C, Ctrl+V and Ctrl+A make, for the mouse, a pen or a touch screen.
//!
//! Every kit text field calls [`menu`] right after drawing its editor, with the text it edits, so
//! the menu acts on that text in the same frame: the caller sees the change in `changed()` as if
//! it were typed, and a field that commits when it loses focus (a sketch dimension, a rename)
//! keeps it, because the menu gives the focus back. Paste goes through the text buffer's own
//! insert with the field's character limit, and a single-line field turns line breaks into
//! spaces, as the keyboard's paste does. A password is never copied.
//!
//! A right press moves egui's cursor to the press, as a left press does; a press inside the
//! selection keeps the selection instead, so Copy and Cut take what was selected.
//!
//! The clipboard's text is read once, when the menu opens, to know whether Paste has anything to
//! paste. The browser only hands its clipboard to a paste the user makes (Ctrl+V), so in the web
//! app Paste stays disabled and the shortcut beside it says how.

use egui::text::{CCursor, CCursorRange};
use egui::text_edit::TextEditState;
use egui::{Response, TextBuffer, Ui};

/// What a field's menu may do with its text, besides what the buffer itself allows
/// (`TextBuffer::is_mutable`: a read-only text offers Copy and Select All only).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Field {
    /// A password: its text is never copied or cut.
    pub secret: bool,
    /// More than one line: a pasted line break stays a line break.
    pub multiline: bool,
    /// The most characters the field holds (`TextEdit::char_limit`).
    pub limit: usize,
}

impl Field {
    /// A single-line field without a character limit.
    pub(crate) const LINE: Self = Self { secret: false, multiline: false, limit: usize::MAX };
    /// A multiline field without a character limit.
    pub(crate) const LINES: Self = Self { secret: false, multiline: true, limit: usize::MAX };

    /// This field, holding at most `limit` characters.
    pub(crate) const fn limited(self, limit: usize) -> Self {
        Self { limit, ..self }
    }
}

/// One entry of the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Edit {
    Cut,
    Copy,
    Paste,
    SelectAll,
}

impl Edit {
    pub(crate) const ALL: [Self; 4] = [Self::Cut, Self::Copy, Self::Paste, Self::SelectAll];

    /// The menu row's id (`menu:<id>` in scenarios), message, icon and shortcut.
    fn row(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            Self::Cut => ("text_cut", "ui.cut", "cut", "Ctrl+X"),
            Self::Copy => ("text_copy", "ui.copy", "copy", "Ctrl+C"),
            Self::Paste => ("text_paste", "ui.paste", "paste", "Ctrl+V"),
            Self::SelectAll => ("text_select_all", "ui.select_all", "", "Ctrl+A"),
        }
    }
}

/// What the field's state allows now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct State {
    /// The text can change (not read-only, not disabled).
    pub editable: bool,
    /// Some characters are selected.
    pub selected: bool,
    /// The text is a password.
    pub secret: bool,
    /// The clipboard holds text to paste.
    pub pastable: bool,
    /// The field holds some text.
    pub text: bool,
}

/// Whether `edit` can be chosen in a field in `state`.
pub(crate) fn enabled(edit: Edit, state: State) -> bool {
    match edit {
        Edit::Cut => state.editable && state.selected && !state.secret,
        Edit::Copy => state.selected && !state.secret,
        Edit::Paste => state.editable && state.pastable,
        Edit::SelectAll => state.text,
    }
}

/// The result of an [`apply`]: the selection after it, the text for the clipboard, and whether
/// the text changed.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Done {
    pub range: CCursorRange,
    pub copied: Option<String>,
    pub changed: bool,
}

/// `range` within a text of `len` characters: a stale selection never reaches past the end.
fn clamp(range: CCursorRange, len: usize) -> CCursorRange {
    let at = |c: CCursor| CCursor::new(c.index.0.min(len));
    CCursorRange { primary: at(range.primary), secondary: at(range.secondary), h_pos: None }
}

/// Make `edit` on `text` with `range` selected (None: the caret at the end), pasting `clip`.
/// Does nothing that [`enabled`] would refuse.
pub(crate) fn apply(text: &mut dyn TextBuffer, range: Option<CCursorRange>, edit: Edit, clip: Option<&str>, field: Field) -> Done {
    let len = text.as_str().chars().count();
    let range = range.map_or_else(|| CCursorRange::one(CCursor::new(len)), |r| clamp(r, len));
    let selected = !range.is_empty();
    let copyable = selected && !field.secret;
    let unchanged = |range, copied| Done { range, copied, changed: false };
    match edit {
        Edit::SelectAll => unchanged(CCursorRange::two(CCursor::new(0), CCursor::new(len)), None),
        Edit::Copy => unchanged(range, copyable.then(|| range.slice_str(text.as_str()).to_owned())),
        Edit::Cut if copyable && text.is_mutable() => {
            let copied = range.slice_str(text.as_str()).to_owned();
            let at = text.delete_selected(&range);
            Done { range: CCursorRange::one(at), copied: Some(copied), changed: true }
        }
        Edit::Paste if text.is_mutable() => {
            let Some(clip) = clip.filter(|c| !c.is_empty()) else { return unchanged(range, None) };
            let before = text.as_str().to_owned();
            let mut at = text.delete_selected(&range);
            let line;
            let insert = if field.multiline {
                clip
            } else {
                line = clip.replace(['\r', '\n'], " ");
                &line
            };
            text.insert_text_at(&mut at, insert, field.limit);
            let changed = text.as_str() != before;
            Done { range: CCursorRange::one(at), copied: None, changed }
        }
        Edit::Cut | Edit::Paste => unchanged(range, None),
    }
}

/// The field's selection a frame ago, while it has focus.
const LAST: &str = "sc_text_menu_last";
/// The selection a right press landed in, kept while the button is down.
const KEPT: &str = "sc_text_menu_kept";
/// The clipboard's text, read when the menu opened.
const CLIP: &str = "sc_text_menu_clip";
/// The field whose menu is open, and the pass that drew it.
const OPEN: &str = "sc_text_menu_open";

/// Whether `field`'s menu is open (drawn this pass). An expression field's suggestions wait
/// meanwhile, rather than cover the menu.
pub(crate) fn menu_open(ctx: &egui::Context, field: egui::Id) -> bool {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<(egui::Id, u64)>(egui::Id::unique(OPEN))).is_some_and(|(id, drawn)| id == field && drawn == pass)
}

#[cfg(test)]
thread_local! {
    /// What the tests' clipboard holds: tests never read the system clipboard.
    pub(crate) static TEST_CLIPBOARD: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The system clipboard's text, if it holds any.
fn clipboard_text() -> Option<String> {
    #[cfg(test)]
    {
        TEST_CLIPBOARD.with(|c| c.borrow().clone())
    }
    #[cfg(all(not(test), not(target_arch = "wasm32")))]
    {
        arboard::Clipboard::new().and_then(|mut c| c.get_text()).ok().filter(|t| !t.is_empty())
    }
    #[cfg(all(not(test), target_arch = "wasm32"))]
    {
        None
    }
}

/// The field's selection now, after keeping the one a right press landed in.
fn keep_selection(ui: &Ui, response: &Response, len: usize) -> Option<CCursorRange> {
    let (ctx, id) = (ui.ctx(), response.id);
    let mut state = TextEditState::load(ctx, id)?;
    let now = state.cursor.char_range().map(|r| clamp(r, len));
    let (pressed, down) = ui.input(|i| (i.pointer.button_pressed(egui::PointerButton::Secondary), i.pointer.secondary_down()));
    let last = ctx.data(|d| d.get_temp::<CCursorRange>(id.with(LAST))).map(|r| clamp(r, len));
    let mut kept = ctx.data(|d| d.get_temp::<CCursorRange>(id.with(KEPT)));
    if pressed && response.contains_pointer() {
        // The press put the caret where it landed; inside the selection, the selection stays.
        let inside = |selection: &CCursorRange| {
            let [min, max] = selection.sorted_cursors();
            now.is_some_and(|n| (min.index..=max.index).contains(&n.primary.index))
        };
        kept = last.filter(|l| !l.is_empty() && inside(l));
    }
    if !down {
        kept = None;
    }
    // Read before `data_mut`: both lock the context's memory.
    let focused = response.has_focus();
    let range = match kept {
        Some(keep) => {
            state.cursor.set_char_range(Some(keep));
            state.store(ctx, id);
            Some(keep)
        }
        // The click on a menu row took the focus before the field drew, and egui 0.37 drops an
        // unfocused field's selection: the row acts on the selection of the last focused frame.
        // Drop once emilk/egui#8763 is fixed.
        None if !focused => last.or(now),
        None => now,
    };
    ctx.data_mut(|d| {
        match kept {
            Some(keep) => {
                d.insert_temp(id.with(KEPT), keep);
            }
            None => d.remove::<CCursorRange>(id.with(KEPT)),
        }
        match range.filter(|_| focused) {
            Some(range) => {
                d.insert_temp(id.with(LAST), range);
            }
            None => d.remove::<CCursorRange>(id.with(LAST)),
        }
    });
    range
}

/// The right-click menu of the text field `response` draws, editing `text`: call it right after
/// adding the field's `TextEdit`. A change made from the menu marks `response` changed.
pub(crate) fn menu(ui: &Ui, response: &mut Response, text: &mut dyn TextBuffer, field: Field) {
    let id = response.id;
    let len = text.as_str().chars().count();
    let range = keep_selection(ui, response, len);
    if response.secondary_clicked() {
        let clip = clipboard_text();
        ui.ctx().data_mut(|d| d.insert_temp(id.with(CLIP), clip));
    }
    let clip = || ui.ctx().data(|d| d.get_temp::<Option<String>>(id.with(CLIP))).flatten();
    let state = State {
        editable: text.is_mutable() && ui.is_enabled(),
        selected: range.is_some_and(|r| !r.is_empty()),
        secret: field.secret,
        pastable: clip().is_some_and(|c| !c.is_empty()),
        text: len > 0,
    };
    let mut picked = None;
    let open = super::Menu::context(response, |ui| {
        super::Menu::show(ui, crate::context_menu::LIST_W, |ui| {
            for edit in Edit::ALL {
                let (row, message, icon, key) = edit.row();
                let shortcut = crate::keymap::display(key);
                let mut item = super::MenuItem::new(row, crate::i18n::tr(message), icon);
                item.shortcut = &shortcut;
                item.enabled = enabled(edit, state);
                if item.show(ui).clicked() && item.enabled {
                    picked = Some(edit);
                    ui.close();
                }
            }
        });
    })
    .is_some();
    if open {
        let pass = ui.ctx().cumulative_pass_nr();
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(OPEN), (id, pass)));
    }
    if open && ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        // Esc closes the menu alone: the field keeps its focus, and its owner (a dimension's box,
        // a dialog) never sees the key that would cancel it.
        ui.ctx().memory_mut(|m| m.request_focus(id));
    }
    let Some(edit) = picked else { return };
    let done = apply(text, range, edit, clip().as_deref(), field);
    let ctx = ui.ctx();
    if let Some(copied) = done.copied {
        ctx.copy_text(copied);
    }
    let mut state = TextEditState::load(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(done.range));
    state.store(ctx, id);
    ctx.data_mut(|d| d.insert_temp(id.with(LAST), done.range));
    // The menu's click took the focus: give it back, so the field neither commits nor closes.
    ctx.memory_mut(|m| m.request_focus(id));
    if done.changed {
        response.mark_changed();
    }
}
