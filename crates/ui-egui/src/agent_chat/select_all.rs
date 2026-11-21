//! Select All in the transcript (#10676): Ctrl+A (Cmd+A) while the chat owns the keyboard and no
//! text box has focus selects the whole retained conversation, and Copy copies it.
//!
//! Every Markdown block of a reply is a label of its own, and egui's label selection cannot be
//! set from code, so its own Ctrl+A reaches only the block the selection is in. Instead the
//! transcript keeps one flag: every prompt and answer is drawn over the selection colour, and Copy
//! puts their text on the clipboard in order, off-screen messages too. Thoughts, tool cards,
//! plans, notices, permission and approval cards and the composer are not part of it. A press of
//! the mouse button or Esc ends it, so a click and drag starts an ordinary partial selection.

use egui::{Event, Key, Modifiers};

use super::{AgentChat, Item, markdown, markdown_view};
use solidcraft_ui_text::tr;

/// The messages Select All takes: what the user sent and what the assistant answered.
fn message(item: &Item) -> Option<(bool, &str)> {
    match item {
        Item::User { text, .. } if !text.is_empty() => Some((true, text)),
        Item::Agent { thought: false, text } if !text.trim().is_empty() => Some((false, text)),
        _ => None,
    }
}

/// The text Copy puts on the clipboard: each message under who wrote it, a reply as it reads on
/// screen (its Markdown laid out as plain text).
pub(super) fn transcript_text(items: &[Item]) -> String {
    let mut out = Vec::new();
    for (user, text) in items.iter().filter_map(message) {
        let (who, body) = if user {
            (tr("ui.agent_chat_transcript_user"), text.trim_end().to_owned())
        } else {
            (tr("ui.agent_chat_transcript_assistant"), markdown_view::plain_text(&markdown::parse(text)))
        };
        out.push(format!("{who}\n{body}"));
    }
    out.join("\n\n")
}

/// Keys and presses for the transcript selection; called each frame the conversation is drawn,
/// in the window (main or the chat's own) whose input it reads.
pub(super) fn handle(chat: &mut AgentChat, ctx: &egui::Context) {
    // The chat's own window is all chat; in the main window a press in the chat claims the keys.
    let owns = ctx.viewport_id() != egui::ViewportId::ROOT || chat.keyboard_owned;
    if !owns || ctx.input(|i| i.pointer.any_pressed() || i.key_pressed(Key::Escape)) || !chat.items.iter().any(|i| message(i).is_some()) {
        chat.all_selected = false;
    }
    // A focused text box (the composer, a queued message) keeps its own Select All and Copy.
    if !owns || ctx.egui_wants_keyboard_input() {
        return;
    }
    if chat.items.iter().any(|i| message(i).is_some()) && ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::A)) {
        chat.all_selected = true;
        // The block selected before would be copied too, and drawn twice.
        ctx.plugin::<egui::text_selection::LabelSelectionState>().lock().clear_selection();
    }
    if chat.all_selected {
        let copy = ctx.input_mut(|i| {
            let before = i.events.len();
            i.events.retain(|e| !matches!(e, Event::Copy | Event::Cut));
            i.events.len() != before
        });
        if copy {
            ctx.copy_text(transcript_text(&chat.items));
        }
    }
}
