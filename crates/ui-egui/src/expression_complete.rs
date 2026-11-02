//! Focus-scoped completion keyboard handling before editors and CAD shortcuts see input.

use crate::ui_kit;
use egui::Key;

use crate::params_dialog::{every_parameter, exact_token, suggestions_for, unit_token, word_before};
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
    /// A row is selected, so Enter and Tab complete it. A popup opened on a word that is
    /// itself a complete name (`width`) starts unarmed: it only shows the longer names, Enter
    /// and Tab reach the editor as if it were not there, and an arrow key arms it (#3663). A
    /// unit means something only after a value: after a number no popup opens, after a name or
    /// constant (`pi mm`) it is a complete name as above (#8862). A unit typed anywhere else
    /// (`rad` on the way to `radius`) is a prefix like any other and the popup is armed.
    /// A press on a row inserts it whether or not the list is armed.
    armed: bool,
    /// A key armed the list (an arrow, or Ctrl+Space). Otherwise `armed` is worked out again
    /// every frame: the names it depends on are gathered only once a box has the focus, and
    /// the text before the word can change under it (`20 mm` replaced by `pi mm`).
    keyed: bool,
    /// The box was empty when last drawn: Down arrow there opens the list.
    empty: bool,
    /// Down arrow in the empty box opened it: every named parameter, however many (#9246).
    every: bool,
    /// The box was typed in since it took the focus. Until then an unarmed list only shows
    /// what longer names there are, for a value the window filled in (`T` in Edit Extrude
    /// lists `tan(`): Escape passes it by and closes the window as it would without the list,
    /// as Enter and Tab do (#9084).
    typed: bool,
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
        (popup.armed, popup.keyed) = (true, true);
        ctx.data_mut(|d| d.insert_temp(popup_id(), popup.clone()));
    }
    // Down arrow in an empty box opens every named parameter (#9246): one need not remember
    // a letter of the name. Once the list is open, Down moves through it as below.
    if popup.empty
        && (popup.count == 0 || popup.dismissed)
        && ctx.memory(|m| m.has_focus(popup.editor))
        && popup.frame.saturating_add(1) >= ctx.cumulative_frame_nr()
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::ArrowDown))
    {
        ctx.input_mut(|i| {
            i.raw.events.retain(|e| !matches!(e, egui::Event::Key { key: Key::ArrowDown, modifiers, .. } if modifiers.is_none()));
        });
        (popup.explicit, popup.every, popup.dismissed) = (true, true, false);
        (popup.armed, popup.keyed, popup.selected) = (true, true, 0);
        ctx.data_mut(|d| d.insert_temp(popup_id(), popup.clone()));
        return;
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
                    Key::ArrowDown if !popup.armed => (popup.armed, popup.keyed, popup.selected) = (true, true, 0),
                    Key::ArrowUp if !popup.armed => (popup.armed, popup.keyed, popup.selected) = (true, true, popup.count - 1),
                    Key::ArrowDown => popup.selected = (popup.selected + 1) % popup.count,
                    Key::ArrowUp => popup.selected = (popup.selected + popup.count - 1) % popup.count,
                    // Unarmed, the typed word is complete: Enter submits it and Tab moves on.
                    Key::Enter | Key::Tab if !popup.armed => continue,
                    Key::Enter | Key::Tab => popup.accept = true,
                    Key::Escape if !popup.armed && !popup.typed => continue,
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

/// Whether `editor`'s suggestions were drawn in the last frame.
#[cfg(test)]
pub(crate) fn showing(ctx: &egui::Context, editor: egui::Id) -> bool {
    ctx.data(|d| d.get_temp::<Popup>(popup_id()))
        .is_some_and(|p| p.editor == editor && p.count > 0 && !p.dismissed && p.frame.saturating_add(1) >= ctx.cumulative_frame_nr())
}

pub(crate) fn complete(ui: &mut egui::Ui, resp: &egui::Response, text: &mut String) -> bool {
    let ctx = ui.ctx().clone();
    // The field's right-click menu is open (#8875): the suggestions wait rather than cover it.
    if ui_kit::text_menu_open(&ctx, resp.id) {
        return false;
    }
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
    let previous = ctx.data(|d| d.get_temp::<Popup>(popup_id())).filter(|p| p.editor == resp.id);
    // Dismissed suggestions stay closed until this box is typed in: another control rewriting
    // its text (a slider dragged while the box keeps the focus) or a moved caret is no reason.
    let dismissed = previous.as_ref().is_some_and(|p| p.dismissed) && !resp.changed();
    let typed = resp.changed() || previous.as_ref().is_some_and(|p| p.typed);
    // What the word follows, blanks aside.
    let before = text.chars().take(start).filter(|c| !c.is_whitespace()).last();
    // A word that is already a complete name opens unarmed: Enter submits it as typed. A unit is
    // a unit after a value it multiplies, a name or constant as much as a number (`pi mm`, #8862);
    // anywhere else (`rad`, half-way to `radius`) it is a prefix, so it stays armed.
    let after_value = before.is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | ')'));
    let armed = !exact_token(&ctx, &word) || (unit_token(&ctx, &word) && !after_value);
    let mut popup = previous.filter(|p| p.word == word && p.cursor == cursor).unwrap_or(Popup {
        editor: resp.id,
        frame: 0,
        word,
        cursor,
        selected: 0,
        count: 0,
        accept: false,
        dismissed,
        explicit: false,
        rows: Vec::new(),
        armed,
        keyed: false,
        empty: false,
        every: false,
        typed,
    });
    popup.typed = typed;
    popup.empty = text.is_empty();
    popup.armed = popup.keyed || armed;
    // Never over a control being dragged: the drag dismisses them.
    if ctx.dragged_id().is_some_and(|id| id != resp.id) {
        popup.dismissed = true;
    }
    // A unit right after a number (`2 in`, `10 mm`, the form every dialog starts its sizes in)
    // is a unit, not the start of a name: no popup there, so nothing sits over the OK button.
    let number_before = before.is_some_and(|c| c.is_ascii_digit() || c == '.' || c == ')');
    let list = if popup.every {
        every_parameter(&ctx)
    } else if !popup.explicit && number_before && unit_token(&ctx, &popup.word) {
        Vec::new()
    } else {
        suggestions_for(&ctx, &popup.word, popup.explicit)
    };
    popup.frame = ctx.cumulative_frame_nr();
    popup.count = list.len();
    popup.selected = popup.selected.min(list.len().saturating_sub(1));
    if list.is_empty() || popup.dismissed {
        // Nothing drawn: a press where the rows were is not on one.
        popup.rows.clear();
        ctx.data_mut(|d| d.insert_temp(popup_id(), popup));
        return false;
    }
    let mut pick = match pressed_row {
        Some(row) => list.get(row).map(|s| s.text.clone()),
        None if popup.accept && popup.armed => list.get(popup.selected).map(|s| s.text.clone()),
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
        popup.armed.then_some(popup.selected),
        &options,
        // Unarmed, the typed word is complete and Enter submits it; an arrow key arms the list.
        // Escape is the window's until the box is typed in.
        crate::i18n::tr_label(if popup.armed {
            "↑ ↓ choose · Enter / Tab complete · Esc dismiss"
        } else if popup.typed {
            "↑ ↓ choose · Enter applies what you typed · Esc dismiss"
        } else {
            "↑ ↓ choose · Enter applies this value"
        }),
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

/// The open popup for tests: (number of rows, armed).
#[cfg(test)]
pub(crate) fn popup_state(ctx: &egui::Context) -> Option<(usize, bool)> {
    ctx.data(|d| d.get_temp::<Popup>(popup_id())).filter(|p| p.count > 0 && !p.dismissed).map(|p| (p.count, p.armed))
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
            for (name, value) in [("width", 40), ("width_extra", 50), ("wing", 20), ("wire", 10), ("radius", 10)] {
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

        fn frames(&mut self, n: usize) {
            for _ in 0..n {
                self.frame(Vec::new());
            }
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

        /// (rows, armed) of the open popup.
        fn popup(&self) -> Option<(usize, bool)> {
            popup_state(&self.ctx)
        }
    }

    /// #8533: a long parameter name widens the completion popup past a narrow editor.
    #[test]
    fn long_parameter_names_widen_the_completion_popup() {
        let short = Editor::new("wi");
        assert!(showing(&short.ctx, short.id));
        let short_w = crate::scenario::rect_of("dialog:suggestions").map(|r| r.width()).unwrap_or(0.0);

        let ctx = crate::fonts::test_context();
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let long = "Driver_MountHole_to_Cover_Shared_Base_Layout_Angle";
        app.run("parameters.add", json!({"name": long, "expression": "8 deg"})).unwrap();
        params_dialog::publish(&mut app, &ctx);
        let id = egui::Id::new("long_name_completion");
        ctx.memory_mut(|m| m.request_focus(id));
        let mut text = "Driver".to_string();
        let mut output = ctx.run_ui(
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 400.0))), ..Default::default() },
            |ui| {
                begin_frame(ui.ctx());
                params_dialog::publish(&mut app, ui.ctx());
                egui::CentralPanel::default().show(ui, |ui| {
                    let response = ui.add(egui::TextEdit::singleline(&mut text).id(id).desired_width(120.0));
                    let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
                    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(text.chars().count()))));
                    state.store(ui.ctx(), id);
                    let _ = complete(ui, &response, &mut text);
                });
            },
        );
        output.textures_delta.clear();
        // A second frame so the popup sees the caret after "Driver".
        let mut output = ctx.run_ui(
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 400.0))), ..Default::default() },
            |ui| {
                begin_frame(ui.ctx());
                params_dialog::publish(&mut app, ui.ctx());
                egui::CentralPanel::default().show(ui, |ui| {
                    let response = ui.add(egui::TextEdit::singleline(&mut text).id(id).desired_width(120.0));
                    let _ = complete(ui, &response, &mut text);
                });
            },
        );
        output.textures_delta.clear();
        assert!(suggestions(&ctx, "Driver").iter().any(|s| s.text == long));
        let long_w = crate::scenario::rect_of("dialog:suggestions").map(|r| r.width()).unwrap_or(0.0);
        assert!(long_w > short_w + 40.0, "long name popup {long_w} should outgrow short {short_w}");
        assert!(long_w > 200.0, "popup should not stay locked to a narrow editor ({long_w})");
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

    /// Text another control writes into the box (a slider drag) leaves dismissed suggestions
    /// closed, though they would offer something for the new word; typing opens them again.
    #[test]
    fn dismissed_suggestions_stay_closed_until_the_box_is_typed_in() {
        let mut e = Editor::new("wid");
        assert!(showing(&e.ctx, e.id));
        e.key(Key::Escape);
        assert!(!showing(&e.ctx, e.id));
        e.text = "wir".into();
        e.frame(Vec::new());
        assert!(!suggestions(&e.ctx, "wir").is_empty());
        assert!(!showing(&e.ctx, e.id), "a rewritten value does not reopen them");
        e.key(Key::Backspace);
        assert_eq!(e.text, "wi");
        assert!(showing(&e.ctx, e.id), "typing does");
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

    /// A word that is itself a complete name submits on Enter and lets Tab move on, whether or
    /// not longer names share its prefix. Those names stay listed, unarmed, so `width` shows
    /// `width_extra` instead of nothing (#3663).
    #[test]
    fn exact_tokens_submit_without_completing_longer_names() {
        for (text, rows) in [("width", true), ("2 mm", false)] {
            for key in [Key::Enter, Key::Tab] {
                let mut e = Editor::new(text);
                let popup = e.popup();
                assert_eq!(popup.is_some(), rows, "{text:?}: {popup:?}");
                if let Some((count, armed)) = popup {
                    assert!(count > 0 && !armed, "{text:?}: {popup:?}");
                }
                e.key(key);
                assert_eq!(e.text, text);
                assert!(!e.accepted);
                assert!(e.leaked.contains(&key), "{text:?} {key:?}");
            }
        }
    }

    /// A list that opened by itself on a value the window filled in leaves Escape to the window
    /// (#9084); once the box is typed in, Escape dismisses the list first.
    #[test]
    fn escape_passes_an_unarmed_list_until_the_box_is_typed_in() {
        let mut e = Editor::new("width");
        assert_eq!(e.popup().map(|(_, armed)| armed), Some(false));
        e.key(Key::Escape);
        assert!(e.leaked.contains(&Key::Escape));
        assert!(!e.ctx.memory(|m| m.has_focus(e.id)));

        let mut e = Editor::new("widt");
        e.frame(vec![egui::Event::Text("h".into())]);
        assert_eq!(e.text, "width");
        assert_eq!(e.popup().map(|(_, armed)| armed), Some(false));
        e.key(Key::Escape);
        assert!(e.leaked.is_empty());
        assert!(!showing(&e.ctx, e.id));
        assert!(e.ctx.memory(|m| m.has_focus(e.id)));
    }

    /// From the unarmed popup an arrow key picks a row (Down the first, Up the last); Enter or
    /// Tab then complete it as usual.
    #[test]
    fn arrows_arm_the_popup_on_an_exact_token() {
        let mut e = Editor::new("width");
        assert!(suggestions(&e.ctx, "width").iter().any(|s| s.text == "width_extra"));
        assert_eq!(e.popup().map(|(_, armed)| armed), Some(false));
        e.key(Key::ArrowDown);
        assert_eq!(e.popup().map(|(_, armed)| armed), Some(true));
        assert_eq!(e.selected(), 0);
        assert!(e.leaked.is_empty());
        e.key(Key::Enter);
        assert_eq!(e.text, "width_extra");
        assert!(e.accepted && e.leaked.is_empty());

        let mut e = Editor::new("2 * width");
        assert_eq!(e.popup().map(|(_, armed)| armed), Some(false));
        e.key(Key::ArrowUp);
        let (count, armed) = e.popup().unwrap();
        assert!(armed);
        assert_eq!(e.selected(), count - 1);
        e.key(Key::Tab);
        assert!(e.accepted && e.text.starts_with("2 * "));
        assert_ne!(e.text, "2 * width");
    }

    /// A unit right after a number is a unit: `10 mm` opens no popup even with a parameter
    /// `mm_wall` defined (a dialog starting on `2 in` otherwise listed names over its OK button),
    /// and Enter submits `10 mm`. After an operator the same word is a prefix again: `2 * mm`
    /// lists `mm_wall`, armed, so Enter completes it.
    #[test]
    fn a_unit_after_a_number_opens_no_popup() {
        let mut e = Editor::new("10 mm");
        assert!(e.popup().is_none());
        e.app.run("parameters.add", json!({"name":"mm_wall","expression":"3 mm"})).unwrap();
        e.frame(Vec::new());
        assert!(e.popup().is_none(), "{:?}", e.popup());
        assert!(suggestions(&e.ctx, "mm").iter().any(|s| s.text == "mm_wall"));
        e.key(Key::Enter);
        assert_eq!(e.text, "10 mm");
        assert!(!e.accepted && e.leaked.contains(&Key::Enter));

        let mut e = Editor::new("2 * mm");
        e.app.run("parameters.add", json!({"name":"mm_wall","expression":"3 mm"})).unwrap();
        e.frame(Vec::new());
        assert_eq!(e.popup(), Some((1, true)));
        e.key(Key::Enter);
        assert_eq!(e.text, "2 * mm_wall");
    }

    /// A unit after a name or constant multiplies it, as after a number: `pi mm` is π mm and
    /// `pi rad` half a turn. The longer names are listed unarmed, so Enter submits what was
    /// typed, and Down then Enter still completes `mm_wall` or `radius` (#8862).
    #[test]
    fn a_unit_after_a_name_submits_as_typed() {
        for (text, longer) in [("pi mm", "pi mm_wall"), ("pi rad", "pi radius")] {
            for arrow in [false, true] {
                let mut e = Editor::new(text);
                e.app.run("parameters.add", json!({"name":"mm_wall","expression":"3 mm"})).unwrap();
                e.frame(Vec::new());
                assert_eq!(e.popup(), Some((1, false)), "{text}");
                if arrow {
                    e.key(Key::ArrowDown);
                }
                e.key(Key::Enter);
                if arrow {
                    assert_eq!(e.text, longer);
                    assert!(e.accepted && e.leaked.is_empty(), "{text}");
                } else {
                    assert_eq!(e.text, text);
                    assert!(!e.accepted && e.leaked.contains(&Key::Enter), "{text}");
                }
            }
        }
    }

    /// Whether Enter completes follows the text as it is now, also when the word and the caret
    /// stay where they were: `pi*mm` replaced by `pi mm` in one go (a paste over the selected
    /// text) leaves `mm` at the same place, and Enter must then submit `pi mm` (#8862).
    #[test]
    fn replacing_the_text_before_a_unit_rearms_the_list() {
        let mut e = Editor::new("pi*mm");
        e.app.run("parameters.add", json!({"name":"mm_wall","expression":"3 mm"})).unwrap();
        e.frame(Vec::new());
        assert_eq!(e.popup(), Some((1, true)), "after `*` a unit is a prefix");
        e.frame(vec![egui::Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }]);
        e.frame(vec![egui::Event::Text("pi mm".into())]);
        assert_eq!(e.cursor(), 5);
        assert_eq!(e.popup(), Some((1, false)));
        e.key(Key::Enter);
        assert_eq!(e.text, "pi mm");
        assert!(!e.accepted && e.leaked.contains(&Key::Enter));
    }

    /// Typing a name letter by letter passes through words that happen to be units: `ra` lists
    /// `radius` selected, and `rad` must keep it selected, so Enter completes the name instead
    /// of submitting one radian into a length (found testing #3663).
    #[test]
    fn a_unit_typed_on_the_way_to_a_name_stays_armed() {
        let mut e = Editor::new("ra");
        assert_eq!(e.popup().map(|(_, armed)| armed), Some(true));
        e.frame(vec![egui::Event::Text("d".into())]);
        assert_eq!(e.text, "rad");
        assert_eq!(e.popup(), Some((1, true)));
        e.key(Key::Enter);
        assert_eq!(e.text, "radius");
        assert!(e.accepted && e.leaked.is_empty());
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

    /// #9246: Down arrow in an empty box lists every named parameter, past the eight rows a
    /// typed word shows; the list scrolls to the chosen row and Enter inserts it. In a box that
    /// already holds text, Down stays the editor's.
    #[test]
    fn down_arrow_in_an_empty_box_lists_every_parameter_and_scrolls() {
        let mut e = Editor::new("");
        for i in 1..=15 {
            e.app.run("parameters.add", json!({"name":format!("p{i:02}"),"expression":"1 mm"})).unwrap();
        }
        e.frame(Vec::new());
        assert_eq!(e.popup(), None, "an empty box shows nothing until asked");
        e.key(Key::ArrowDown);
        e.frame(Vec::new());
        assert!(!e.leaked.contains(&Key::ArrowDown));
        assert_eq!(e.popup(), Some((20, true)), "every parameter, armed on the first");
        assert_eq!(e.selected(), 0);
        assert!(e.text.is_empty());

        e.key(Key::ArrowUp);
        e.frames(2);
        assert_eq!(e.selected(), 19);
        let rows = e.ctx.data(|d| d.get_temp::<Popup>(popup_id())).unwrap().rows;
        assert!(rows.last().is_some_and(|r| r.area() > 0.0), "the chosen last row is scrolled into view");
        assert!(rows.first().is_some_and(|r| r.area() <= 0.0), "the first row has scrolled out: {:?}", rows.first());
        let last = params_dialog::every_parameter(&e.ctx).last().map(|s| s.text.clone()).unwrap();
        e.key(Key::Enter);
        assert!(e.accepted);
        assert_eq!(e.text, last);

        let mut typed = Editor::new("width + ");
        typed.key(Key::ArrowDown);
        assert!(typed.leaked.contains(&Key::ArrowDown), "a box with text keeps Down for itself");
        assert_eq!(typed.popup(), None);

        // Preferences caps the list, so a large design does not list thousands; a hand-edited
        // cap below the accepted range still lists a few.
        for (cap, listed) in [(12, 12), (0, 5)] {
            let mut capped = Editor::new("");
            for i in 1..=15 {
                capped.app.run("parameters.add", json!({"name":format!("p{i:02}"),"expression":"1 mm"})).unwrap();
            }
            capped.app.preferences.parameter_list_rows = cap;
            capped.key(Key::ArrowDown);
            capped.frame(Vec::new());
            assert_eq!(capped.popup(), Some((listed, true)), "cap {cap}");
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
