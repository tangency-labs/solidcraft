//! Markdown export for the conversation currently retained by Assistant.

use egui::Id;
use solidcraft_theme::Tokens;
use solidcraft_ui_text::tr;

use super::panel::PanelAction;
use super::{AgentChat, Item, ToolStatus};
use crate::ui_kit;

pub(super) fn export_dialog(chat: &mut AgentChat, ctx: &egui::Context) {
    let valid = chat.can_export_conversation();
    let Some(include_tools) = chat.export_tools.as_mut() else { return };
    let (closed, action) = ui_kit::Confirm::new(Id::unique("sc_assistant_export"), "export", tr("ui.agent_chat_export").trim_end_matches('…'))
        .actions(tr("ui.save"), tr("ui.cancel"), None)
        .show(ctx, valid, |ui| {
            let checkbox = ui_kit::checkbox(ui, tr("ui.agent_chat_export_tools"), include_tools);
            solidcraft_ui_registry::publish_target("agent_chat:export_tools", checkbox.rect);
            if *include_tools {
                ui.add_space(Tokens::get().space_8);
                ui_kit::text(ui, tr("ui.agent_chat_export_warning"), ui_kit::Text::Report("assistant_export_warning", false));
            }
        });
    for (handle, button) in [("agent_chat:export_save", "dialog:ok"), ("agent_chat:export_cancel", "dialog:cancel")] {
        if let Some(rect) = solidcraft_ui_registry::rect_of(button) {
            solidcraft_ui_registry::publish_target(handle, rect);
        }
    }
    if !closed && action == ui_kit::Action::Ok {
        chat.actions.push(PanelAction::ExportConversation { include_tools: *include_tools });
    }
    if closed || action != ui_kit::Action::None {
        chat.export_tools = None;
    }
}

/// Export messages in display order. Tool calls and notices require a separate choice.
pub(super) fn markdown_snapshot(items: &[Item], include_tools: bool) -> String {
    let mut output =
        String::from("# SolidCraft Assistant conversation\n\n> Snapshot of the retained conversation. Earlier messages may have been discarded.\n\n");
    if include_tools {
        output.push_str(
            "> This export includes retained tool calls, results and notices. Review it for paths and other sensitive data before sharing.\n\n",
        );
    }

    for item in items {
        match item {
            Item::User { text, pictures, files, .. } => {
                push_message(&mut output, "User", text);
                if !files.is_empty() || !pictures.is_empty() {
                    output.push_str("Attachments:\n\n");
                    for name in files {
                        output.push_str("- File: ");
                        push_literal(&mut output, name);
                        output.push('\n');
                    }
                    for picture in pictures {
                        output.push_str("- Image: ");
                        push_literal(&mut output, if picture.view { "Model view" } else { &picture.note });
                        output.push('\n');
                    }
                    output.push('\n');
                }
            }
            Item::Agent { thought: false, text } => push_message(&mut output, "Assistant", text),
            Item::Tool { id, title, kind, status, input, output: result } if include_tools => {
                output.push_str("## Tool call: ");
                push_literal(&mut output, if title.is_empty() { "Untitled" } else { title });
                output.push_str("\n\n- Call ID: ");
                push_literal(&mut output, id);
                output.push_str("\n- Kind: ");
                push_literal(&mut output, if kind.is_empty() { "Tool" } else { kind });
                output.push_str("\n- Status: ");
                output.push_str(match status {
                    Some(ToolStatus::Pending) => "Pending",
                    Some(ToolStatus::InProgress) => "In progress",
                    Some(ToolStatus::Completed) => "Completed",
                    Some(ToolStatus::Failed) => "Failed",
                    None => "Not reported",
                });
                output.push_str("\n\n");
                // Retention limits the compact JSON size. Indentation can greatly expand nested input.
                let arguments = input.as_ref().map(serde_json::Value::to_string);
                push_tool_field(&mut output, "Input", arguments.as_deref(), "json");
                push_tool_field(&mut output, "Output", result.as_deref(), "");
            }
            Item::Notice { text, error } if include_tools => {
                output.push_str(if *error { "## Notice (error)\n\n" } else { "## Notice\n\n" });
                push_fenced(&mut output, text, "");
            }
            Item::Agent { thought: true, .. }
            | Item::Tool { .. }
            | Item::Notice { .. }
            | Item::Plan(_)
            | Item::Permission { .. }
            | Item::Approval { .. }
            | Item::Changes { .. } => {}
        }
    }
    output
}

/// Keep the original Markdown verbatim, even if the message is only whitespace.
fn push_message(output: &mut String, role: &str, text: &str) {
    output.push_str("## ");
    output.push_str(role);
    output.push_str("\n\n");
    output.push_str(text);
    if let Some((indent, marker, length)) = open_fence(text) {
        // A partial reply can end in a code block. End the block after the retained text so
        // subsequent headings do not render as part of the reply.
        if !text.ends_with('\n') {
            output.push('\n');
        }
        output.extend(std::iter::repeat_n(' ', indent));
        output.extend(std::iter::repeat_n(char::from(marker), length));
    }
    output.push_str("\n\n");
}

/// Find an unclosed Markdown code fence with at most three leading spaces.
fn open_fence(text: &str) -> Option<(usize, u8, usize)> {
    let mut open = None;
    for raw in text.split(['\n', '\r']) {
        let line = raw.trim_start_matches(' ');
        let indent = raw.len() - line.len();
        if indent > 3 {
            continue;
        }
        let Some(marker @ (b'`' | b'~')) = line.as_bytes().first().copied() else { continue };
        let length = line.bytes().take_while(|&byte| byte == marker).count();
        if length < 3 {
            continue;
        }
        let rest = &line[length..];
        if let Some((_, opened_with, opened_length)) = open {
            if marker == opened_with && length >= opened_length && rest.trim_matches([' ', '\t']).is_empty() {
                open = None;
            }
        } else if marker == b'~' || !rest.contains('`') {
            open = Some((indent, marker, length));
        }
    }
    open
}

/// Display a retained label as literal text within one Markdown line.
fn push_literal(output: &mut String, label: &str) {
    for ch in label.chars() {
        if ch.is_control() {
            output.extend(ch.escape_debug());
        } else {
            if matches!(ch, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '&' | '~' | '#') {
                output.push('\\');
            }
            output.push(ch);
        }
    }
}

/// Missing and empty tool data are different. A present value is exported in full.
fn push_tool_field(output: &mut String, label: &str, text: Option<&str>, lang: &str) {
    output.push_str(label);
    if let Some(text) = text {
        output.push_str(":\n\n");
        push_fenced(output, text, lang);
    } else {
        output.push_str(": not retained.\n\n");
    }
}

/// Use a fence longer than any backtick run in its contents.
fn push_fenced(output: &mut String, text: &str, lang: &str) {
    let longest = text.split(|ch| ch != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.saturating_add(1).max(3));
    output.push_str(&fence);
    output.push_str(lang);
    output.push('\n');
    output.push_str(text);
    if !text.is_empty() && !text.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(&fence);
    output.push_str("\n\n");
}
