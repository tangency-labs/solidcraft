//! Focus-scoped completion keyboard handling before editors and CAD shortcuts see input.

use crate::ui_kit;
use egui::Key;

use crate::params_dialog::{suggestions_for, word_before};
use crate::theme::Tokens;

#[derive(Clone)]
struct Popup {
    editor: egui::Id,
    frame: u64,
    word: String,
    cursor: usize,
    selected: usize,
    count: usize,
    accept: bool,
    dismissed: bool,
    explicit: bool,
    /// Where each suggestion was drawn, for a press on one (see [`complete`]).
    rows: Vec<egui::Rect>,
}

fn popup_id() -> egui::Id {
    egui::Id::new("expression_completion_keyboard")
}

/// Run before any TextEdit or viewport shortcuts. Only the focused, recently drawn popup
/// owns these keys. Raw events are removed too: several expression editors inspect raw Tab.
pub(crate) fn begin_frame(ctx: &egui::Context) {
    let Some(mut popup) = ctx.data(|d| d.get_temp::<Popup>(popup_id())) else { return };
    if ctx.memory(|m| m.has_focus(popup.editor))
        && popup.frame.saturating_add(1) >= ctx.cumulative_frame_nr()
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::Space))
    {
        ctx.input_mut(|i| {
            i.raw.events.retain(|e| !matches!(e, egui::Event::Key { key: Key::Space, modifiers, .. } if modifiers.ctrl));
        });
        popup.explicit = true;
        popup.dismissed = false;
        ctx.data_mut(|d| d.insert_temp(popup_id(), popup.clone()));
    }
    let escape_blurred = ctx.input(|i| i.key_pressed(Key::Escape)) && ctx.memory(|m| m.focused().is_none() && m.had_focus_last_frame(popup.editor));
    let focused = ctx.memory(|m| m.has_focus(popup.editor)) || escape_blurred;
    if popup.frame.saturating_add(1) < ctx.cumulative_frame_nr() || !focused || popup.dismissed || popup.count == 0 {
        if !focused || popup.frame.saturating_add(1) < ctx.cumulative_frame_nr() {
            ctx.data_mut(|d| d.remove::<Popup>(popup_id()));
        }
        return;
    }
    // Let the editor process a newly typed/deleted prefix before deciding what Enter means.
    // A prior frame's completion must never replace text changed in this input batch.
    if ctx.input(|i| {
        i.raw.events.iter().any(|e| {
            matches!(e, egui::Event::Text(_) | egui::Event::Paste(_) | egui::Event::Key { key: Key::Backspace | Key::Delete, pressed: true, .. })
        })
    }) {
        return;
    }
    let mut consumed = false;
    ctx.input_mut(|input| {
        let mut owned = Vec::new();
        for event in &input.raw.events {
            if let egui::Event::Key { key, pressed: true, modifiers, .. } = event
                && *modifiers == egui::Modifiers::NONE
            {
                match key {
                    Key::ArrowDown => popup.selected = (popup.selected + 1) % popup.count,
                    Key::ArrowUp => popup.selected = (popup.selected + popup.count - 1) % popup.count,
                    Key::Enter | Key::Tab => popup.accept = true,
                    Key::Escape => {
                        popup.dismissed = true;
                        popup.accept = false;
                    }
                    _ => continue,
                }
                owned.push(*key);
                consumed = true;
            }
        }
        let keep = |event: &egui::Event| {
            !matches!(event, egui::Event::Key { key, modifiers, .. }
            if owned.contains(key) && *modifiers == egui::Modifiers::NONE)
        };
        input.events.retain(keep);
        input.raw.events.retain(keep);
    });
    if consumed {
        // egui schedules navigation (and Escape blur) at begin_pass, before this hook.
        ctx.memory_mut(|m| {
            m.move_focus(egui::FocusDirection::None);
            m.request_focus(popup.editor);
        });
    }
    ctx.data_mut(|d| d.insert_temp(popup_id(), popup));
}

pub(crate) fn complete(ui: &mut egui::Ui, resp: &egui::Response, text: &mut String) -> bool {
    let ctx = ui.ctx().clone();
    // A press on a suggestion takes the focus from the editor before the list can see it
    // clicked, and the editor then gives up what was typed (#3257). The press picks the
    // suggestion under it, and the editor gets its focus back below.
    let pressed_row = ctx
        .input(|i| i.pointer.primary_pressed().then(|| i.pointer.interact_pos()).flatten())
        .and_then(|at| ctx.data(|d| d.get_temp::<Popup>(popup_id())).filter(|p| p.editor == resp.id)?.rows.iter().position(|r| r.contains(at)));
    // Its release is a click away from the editor, which gives up the focus again: take it back,
    // and don't let the list take that release as a click on another row.
    let picked_by_press = resp.id.with("completion_picked_by_press");
    let released_after_press = ctx.data(|d| d.get_temp::<bool>(picked_by_press)).is_some() && ctx.input(|i| i.pointer.any_released());
    if released_after_press {
        ctx.data_mut(|d| d.remove::<bool>(picked_by_press));
        resp.request_focus();
    }
    if !ctx.memory(|m| m.has_focus(resp.id)) && pressed_row.is_none() {
        return false;
    }
    let state = egui::TextEdit::load_state(&ctx, resp.id);
    let cursor = state.as_ref().and_then(|s| s.cursor.char_range()).map(|r| r.primary.index.0).unwrap_or_else(|| text.chars().count());
    let (start, word) = word_before(text, cursor);
    let previous = ctx.data(|d| d.get_temp::<Popup>(popup_id()));
    let mut popup = previous.filter(|p| p.editor == resp.id && p.word == word && p.cursor == cursor).unwrap_or(Popup {
        editor: resp.id,
        frame: 0,
        word,
        cursor,
        selected: 0,
        count: 0,
        accept: false,
        dismissed: false,
        explicit: false,
        rows: Vec::new(),
    });
    let list = suggestions_for(&ctx, &popup.word, popup.explicit);
    popup.frame = ctx.cumulative_frame_nr();
    popup.count = list.len();
    popup.selected = popup.selected.min(list.len().saturating_sub(1));
    if list.is_empty() || popup.dismissed {
        ctx.data_mut(|d| d.insert_temp(popup_id(), popup));
        return false;
    }
    let mut pick = match pressed_row {
        Some(row) => list.get(row).map(|s| s.text.clone()),
        None if popup.accept => list.get(popup.selected).map(|s| s.text.clone()),
        None => None,
    };
    popup.accept = false;
    let t = Tokens::get();
    let options: Vec<_> = list
        .iter()
        .map(|suggestion| ui_kit::PopupOption {
            text: &suggestion.text,
            kind: match suggestion.kind.as_str() {
                "parameter" => crate::i18n::tr_label("Parameter"),
                "function" => crate::i18n::tr("ui.function"),
                "unit" => crate::i18n::tr_label("Unit"),
                "constant" => crate::i18n::tr("ui.constant"),
                _ => &suggestion.kind,
            },
            detail: &suggestion.detail,
            color: match suggestion.kind.as_str() {
                "parameter" => t.form_ink,
                "function" => t.form_primary,
                _ => t.form_dim,
            },
        })
        .collect();
    let list_id = resp.id.with("completions");
    if let Some(index) = ui_kit::popup_list(
        &ctx,
        list_id,
        resp.rect,
        popup.selected,
        &options,
        crate::i18n::tr_label("↑ ↓ choose · Enter / Tab complete · Esc dismiss"),
    ) && pick.is_none()
        && !released_after_press
    {
        pick = list.get(index).map(|suggestion| suggestion.text.clone());
    }
    popup.rows = ctx.data(|d| d.get_temp::<Vec<egui::Rect>>(list_id.with("rows"))).unwrap_or_default();
    ctx.data_mut(|d| d.insert_temp(popup_id(), popup));
    let Some(pick) = pick else { return false };
    let chars: Vec<char> = text.chars().collect();
    let before: String = chars.get(..start).map(|s| s.iter().collect()).unwrap_or_default();
    let after: String = chars.get(cursor.min(chars.len())..).map(|s| s.iter().collect()).unwrap_or_default();
    *text = format!("{before}{pick}{after}");
    let mut state = state.unwrap_or_default();
    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(start + pick.chars().count()))));
    state.store(&ctx, resp.id);
    resp.request_focus();
    if pressed_row.is_some() {
        ctx.data_mut(|d| d.insert_temp(picked_by_press, true));
    }
    // Do not leave the accepted popup owning Enter on the next frame.
    ctx.data_mut(|d| d.remove::<Popup>(popup_id()));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params_dialog::suggestions;
    use crate::{SolveApp, params_dialog};
    use serde_json::json;

    struct Editor {
        ctx: egui::Context,
        app: SolveApp,
        text: String,
        id: egui::Id,
        accepted: bool,
        leaked: Vec<Key>,
    }

    impl Editor {
        fn new(text: &str) -> Self {
            let ctx = crate::fonts::test_context();
            let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
            for (name, value) in [("width", 40), ("width_extra", 50), ("wing", 20), ("wire", 10)] {
                app.run("parameters.add", json!({"name":name,"expression":format!("{value} mm")})).unwrap();
            }
            params_dialog::publish(&mut app, &ctx);
            let id = egui::Id::new("completion_test_editor");
            ctx.memory_mut(|m| m.request_focus(id));
            let mut editor = Self { ctx, app, text: text.into(), id, accepted: false, leaked: Vec::new() };
            editor.frame(Vec::new());
            let mut state = egui::TextEdit::load_state(&editor.ctx, id).unwrap();
            state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(text.chars().count()))));
            state.store(&editor.ctx, id);
            editor.frame(Vec::new());
            editor
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            self.accepted = false;
            self.leaked.clear();
            let mut output = self.ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 400.0))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    begin_frame(ui.ctx());
                    params_dialog::publish(&mut self.app, ui.ctx());
                    egui::CentralPanel::default().show(ui, |ui| {
                        let response = ui.add(egui::TextEdit::singleline(&mut self.text).id(self.id));
                        self.accepted = complete(ui, &response, &mut self.text);
                        for key in [Key::ArrowUp, Key::ArrowDown, Key::Enter, Key::Tab, Key::Escape] {
                            if ui.input(|i| {
                                i.key_pressed(key)
                                    || i.raw.events.iter().any(|e| matches!(e, egui::Event::Key { key:k, pressed:true, .. } if *k == key))
                            }) {
                                self.leaked.push(key);
                            }
                        }
                    });
                },
            );
            // Nothing paints these frames: drop their texture updates on purpose.
            output.textures_delta.clear();
        }

        fn key(&mut self, key: Key) {
            self.frame(vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }]);
        }

        fn cursor(&self) -> usize {
            egui::TextEdit::load_state(&self.ctx, self.id).unwrap().cursor.char_range().unwrap().primary.index.0
        }

        fn selected(&self) -> usize {
            self.ctx.data(|d| d.get_temp::<Popup>(popup_id())).unwrap().selected
        }
    }

    #[test]
    fn arrows_cycle_without_moving_caret_enter_accepts_selected_without_leaking() {
        let mut e = Editor::new("2 * wi");
        let list = suggestions(&e.ctx, "wi");
        assert!(list.len() >= 3);
        let cursor = e.cursor();
        e.key(Key::ArrowDown);
        assert_eq!(e.selected(), 1);
        assert_eq!(e.cursor(), cursor);
        assert!(e.leaked.is_empty());
        e.key(Key::ArrowUp);
        assert_eq!(e.selected(), 0);
        e.key(Key::ArrowUp);
        assert_eq!(e.selected(), list.len() - 1);
        e.key(Key::Enter);
        assert_eq!(e.text, format!("2 * {}", list.last().unwrap().text));
        assert!(e.accepted);
        assert!(e.leaked.is_empty());
        assert_eq!(e.cursor(), e.text.chars().count());
        assert!(e.ctx.memory(|m| m.has_focus(e.id)));
    }

    /// #3257: a click on a suggestion inserts it, though the press takes the focus from the
    /// editor first; the editor keeps its focus and its caret goes after the inserted name.
    #[test]
    fn clicking_a_suggestion_inserts_it() {
        let mut e = Editor::new("2 * wi");
        let list = suggestions(&e.ctx, "wi");
        let rows = e.ctx.data(|d| d.get_temp::<Popup>(popup_id())).unwrap().rows;
        assert_eq!(rows.len(), list.len());
        let at = rows[1].center();
        let button =
            |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        e.frame(vec![egui::Event::PointerMoved(at)]);
        e.frame(vec![button(true)]);
        assert!(e.accepted, "the press picked the suggestion under it");
        e.frame(vec![button(false)]);
        assert_eq!(e.text, format!("2 * {}", list[1].text));
        assert!(e.ctx.memory(|m| m.has_focus(e.id)), "the editor keeps the focus");
        assert_eq!(e.cursor(), e.text.chars().count());
    }

    #[test]
    fn tab_accepts_selection_filtering_resets_it_and_escape_dismisses() {
        let mut e = Editor::new("wi");
        e.key(Key::ArrowDown);
        let expected = suggestions(&e.ctx, "wi").get(1).unwrap().text.clone();
        e.key(Key::Tab);
        assert_eq!(e.text, expected);
        assert!(e.accepted && e.leaked.is_empty());
        let mut e = Editor::new("wi");
        e.key(Key::ArrowDown);
        e.frame(vec![egui::Event::Text("d".into())]);
        assert_eq!(e.text, "wid");
        assert_eq!(e.selected(), 0);
        e.key(Key::Escape);
        assert!(e.leaked.is_empty());
        assert_eq!(e.text, "wid");
        e.key(Key::Enter);
        assert!(!e.accepted);
        assert!(e.leaked.contains(&Key::Enter));
    }

    #[test]
    fn absent_popup_preserves_editor_and_application_keys() {
        let mut e = Editor::new("123");
        e.key(Key::ArrowDown);
        assert!(e.leaked.contains(&Key::ArrowDown));
        e.key(Key::Escape);
        assert!(e.leaked.contains(&Key::Escape));
        assert!(!e.ctx.memory(|m| m.has_focus(e.id)));
        e.ctx.memory_mut(|m| m.request_focus(e.id));
        e.frame(Vec::new());
        let mut state = egui::TextEdit::load_state(&e.ctx, e.id).unwrap();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(e.text.chars().count()))));
        state.store(&e.ctx, e.id);
        e.frame(vec![egui::Event::Text("4".into())]);
        assert_eq!(e.text, "1234");
    }

    #[test]
    fn exact_tokens_submit_without_completing_longer_names() {
        for text in ["width", "2 mm"] {
            let mut e = Editor::new(text);
            e.key(Key::Enter);
            assert_eq!(e.text, text);
            assert!(!e.accepted);
            assert!(e.leaked.contains(&Key::Enter));
        }
    }

    #[test]
    fn typing_and_enter_in_one_batch_does_not_accept_stale_prefix() {
        let mut e = Editor::new("wi");
        e.frame(vec![
            egui::Event::Text("dth".into()),
            egui::Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE },
        ]);
        assert_eq!(e.text, "width");
        assert!(!e.accepted);
        assert!(e.leaked.contains(&Key::Enter));
    }

    #[test]
    fn ctrl_space_opens_empty_expression_suggestions_and_reopens_dismissed_ones() {
        for prefix in ["", "wi"] {
            let mut e = Editor::new(prefix);
            if !prefix.is_empty() {
                e.key(Key::Escape);
            }
            e.frame(vec![egui::Event::Key { key: Key::Space, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::CTRL }]);
            let popup = e.ctx.data(|d| d.get_temp::<Popup>(popup_id())).unwrap();
            assert!(popup.explicit && !popup.dismissed && popup.count > 0);
            assert_eq!(e.text, prefix, "Ctrl+Space does not add a space");
            e.key(Key::Tab);
            assert!(e.accepted);
            assert!(!e.text.is_empty());
        }
    }

    #[test]
    fn modified_navigation_remains_available_to_the_editor() {
        let mut e = Editor::new("wi");
        e.frame(vec![egui::Event::Key {
            key: Key::ArrowUp,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers { ctrl: true, ..Default::default() },
        }]);
        assert!(e.leaked.contains(&Key::ArrowUp));
        assert!(!e.accepted);
        let mut e = Editor::new("wi");
        e.frame(vec![egui::Event::Key {
            key: Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers { shift: true, ..Default::default() },
        }]);
        assert!(e.leaked.contains(&Key::Tab));
        assert_eq!(e.text, "wi");
        assert!(!e.accepted);
    }
}
