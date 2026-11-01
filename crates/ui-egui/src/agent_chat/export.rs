//! Export the conversation the chat currently retains as a Markdown snapshot (#10677).
//!
//! The snapshot is built at the moment the action is chosen, before the save dialog opens, so a
//! reply that is still streaming cannot change the file half-written. What the chat no longer
//! retains stays out: the exporter adds no truncation of its own, and never reconstructs tool
//! input or output it does not have — it says the content is not retained instead.
//!
//! Out of both exports: thoughts (even while visible), plans, permission and approval cards
//! (pending or answered), Changes/undo snapshots, context chips and the unsent draft. Notices
//! appear only in the tool-inclusive export, labelled and in their sequence. Retained attachment
//! display names are listed; no binary files are bundled and no extra file-system data is read.

use crate::SolveApp;
use crate::i18n::format as trf;

use super::{Item, ToolStatus};

/// The name the save dialog offers.
const FILE_NAME: &str = "solidcraft-conversation.md";

/// Take the snapshot now and ask where to save it. A cancel changes nothing; a write failure is
/// reported in the status line, and neither touches the chat or the design.
pub(crate) fn start(app: &mut SolveApp, include_tools: bool) {
    // Snapshot before the dialog: a streaming reply keeps appending to `items`, not to this text.
    let text = markdown(&app.agent_chat.items, include_tools);
    app.pick_save(FILE_NAME, &["md"], move |app, path| {
        let Some(path) = path else { return };
        match std::fs::write(&path, text.as_bytes()) {
            Ok(()) => app.set_status(trf("ui.agent_chat_export_saved", &[("path", &path)]), false),
            Err(error) => app.set_status(trf("ui.agent_chat_export_failed", &[("error", &error.to_string())]), true),
        }
    });
}

/// The Markdown for the retained `items`. `include_tools` also brings in notices, per the issue:
/// notices only make sense next to the tool calls they came with.
pub(crate) fn markdown(items: &[Item], include_tools: bool) -> String {
    let mut out = String::new();
    out.push_str("# SolidCraft Assistant conversation\n\n");
    out.push_str("> Snapshot of the conversation currently retained in the Assistant chat.\n");
    out.push_str("> Messages the chat already discarded to stay within its retention limit are not\n");
    out.push_str("> included, and their number is not guessed: this is not a guaranteed full session.\n");
    if include_tools {
        out.push_str(">\n> This export includes tool calls, their results and notices as retained.\n");
        out.push_str("> Retained tool and user content can itself contain file paths or other\n");
        out.push_str("> sensitive data; review it before sharing.\n\n");
    } else {
        out.push_str(">\n> Tool calls and results are not included.\n\n");
    }
    for item in items {
        match item {
            Item::User { text, pictures, files, .. } => {
                out.push_str("## User\n\n");
                let mut names: Vec<&str> = files.iter().map(String::as_str).collect();
                names.extend(pictures.iter().map(|p| p.note.as_str()));
                if text.trim().is_empty() && names.is_empty() {
                    out.push_str("(empty message)\n");
                } else if !text.trim().is_empty() {
                    push_message(&mut out, text);
                }
                if !names.is_empty() {
                    out.push_str(&format!("\nAttachments: {}\n", names.join(", ")));
                }
                out.push('\n');
            }
            Item::Agent { thought: false, text } => {
                out.push_str("## Assistant\n\n");
                push_message(&mut out, text);
                out.push('\n');
            }
            // Thoughts, plans, permission and approval cards, and Changes snapshots stay out.
            Item::Agent { thought: true, .. } | Item::Plan(_) | Item::Permission { .. } | Item::Approval { .. } | Item::Changes { .. } => {}
            Item::Tool { title, kind, status, input, output, .. } => {
                if !include_tools {
                    continue;
                }
                out.push_str(&format!("### Tool call: {}\n\n", if title.trim().is_empty() { "(untitled)" } else { title.trim() }));
                out.push_str(&format!("- Kind: `{}`\n", if kind.trim().is_empty() { "tool" } else { kind.trim() }));
                out.push_str(&format!("- Status: {}\n", status_label(*status)));
                match input {
                    Some(value) => {
                        out.push_str("\nInput:\n\n");
                        push_fence(&mut out, &pretty_json(value), "json");
                        out.push('\n');
                    }
                    None => out.push_str("\nInput: not retained.\n"),
                }
                match output {
                    Some(text) => {
                        out.push_str("\nOutput:\n\n");
                        push_fence(&mut out, text, "");
                        out.push('\n');
                    }
                    None => out.push_str("\nOutput: not retained.\n"),
                }
                out.push('\n');
            }
            Item::Notice { text, error } => {
                if !include_tools {
                    continue;
                }
                out.push_str(&format!("> **Notice{}:** {}\n\n", if *error { " (error)" } else { "" }, text.trim()));
            }
        }
    }
    out
}

/// A message's text verbatim: it is already Markdown, so paragraphs, lists and code fences come
/// through exactly as retained — the exporter adds no fence of its own and truncates nothing.
fn push_message(out: &mut String, text: &str) {
    out.push_str(text.trim_end());
    out.push('\n');
}

/// Wrap retained tool content in a fence that its own content cannot break out of.
fn push_fence(out: &mut String, content: &str, lang: &str) {
    let longest = content.match_indices('`').map(|(i, _)| content[i..].chars().take_while(|c| *c == '`').count()).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(3) + if longest >= 3 { 1 } else { 0 });
    out.push_str(&fence);
    out.push_str(lang);
    out.push('\n');
    out.push_str(content.trim_end());
    out.push('\n');
    out.push_str(&fence);
    out.push('\n');
}

fn status_label(status: Option<ToolStatus>) -> &'static str {
    match status {
        Some(ToolStatus::Pending) => "pending",
        Some(ToolStatus::InProgress) => "in progress",
        Some(ToolStatus::Completed) => "completed",
        Some(ToolStatus::Failed) => "failed",
        None => "not reported",
    }
}

fn pretty_json(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_chat::backend::PermissionOption;
    use crate::agent_chat::context::Context;
    use crate::agent_chat::{AgentChat, Item, Picture, State};
    use serde_json::json;
    use std::sync::Arc;

    const REPLY: &str = "Two steps.\n\n- first\n- second\n\n```rust\nlet x = 1;\n```\n";

    fn sample_items() -> Vec<Item> {
        vec![
            Item::User {
                text: "make a box\n\nwith a hole".into(),
                image: false,
                pictures: vec![Picture::new(vec![1, 2, 3], "view.png")],
                context: Context::default(),
                files: vec!["sample.step".into()],
            },
            Item::Agent { thought: true, text: "hmm, a box".into() },
            Item::Tool {
                id: "t1".into(),
                title: "solid.box".into(),
                kind: "tool_call".into(),
                status: Some(ToolStatus::Completed),
                input: Some(json!({"length": 10})),
                output: Some("ok\n```not a fence break```".into()),
            },
            Item::Tool { id: "t2".into(), title: "".into(), kind: "".into(), status: None, input: None, output: None },
            Item::Notice { text: "the agent restarted".into(), error: false },
            Item::Plan(vec![("do it".into(), "completed".into())]),
            Item::Permission {
                key: 1,
                title: "run a command?".into(),
                options: vec![PermissionOption { id: "yes".into(), name: "Allow".into(), kind: "allow_once".into() }],
                answer: Some("Allow".into()),
            },
            Item::Approval { id: 2, title: "replace the design".into(), tool: "file.replace".into(), answer: Some(true) },
            Item::Changes { steps: 1, doc: Default::default(), undone: false },
            Item::Agent { thought: false, text: REPLY.into() },
            Item::Notice { text: "a write failed".into(), error: true },
        ]
    }

    #[test]
    fn roles_order_and_full_text_are_preserved() {
        let md = markdown(&sample_items(), false);
        let user = md.find("## User").unwrap();
        let assistant = md.find("## Assistant").unwrap();
        assert!(user < assistant, "chronological order");
        assert!(md.contains("make a box\n\nwith a hole"), "user text verbatim");
        assert!(md.contains(REPLY.trim_end()), "assistant text verbatim, with list and code fence");
        assert!(!md.contains("hmm, a box"), "thoughts stay out even while visible");
    }

    #[test]
    fn header_says_snapshot_not_full_session() {
        for tools in [false, true] {
            let md = markdown(&sample_items(), tools);
            assert!(md.starts_with("# SolidCraft Assistant conversation"));
            assert!(md.contains("retained"), "identifies the snapshot");
            assert!(md.contains("not a guaranteed full session"));
        }
    }

    #[test]
    fn tools_are_omitted_by_default() {
        let md = markdown(&sample_items(), false);
        assert!(!md.contains("solid.box"));
        assert!(!md.contains("### Tool call"));
        assert!(!md.contains("the agent restarted"), "notices only in the tool-inclusive export");
        assert!(md.contains("Tool calls and results are not included"));
    }

    #[test]
    fn tool_inclusive_export_labels_calls_and_keeps_content_whole() {
        let md = markdown(&sample_items(), true);
        assert!(md.contains("### Tool call: solid.box"));
        assert!(md.contains("- Kind: `tool_call`"));
        assert!(md.contains("- Status: completed"));
        assert!(md.contains("\"length\": 10"), "input JSON as retained");
        assert!(md.contains("ok\n```not a fence break```"), "output verbatim, fence escaped");
        assert!(md.contains("````"), "a longer fence wraps content holding fences");
        assert!(md.contains("> **Notice:** the agent restarted"));
        assert!(md.contains("> **Notice (error):** a write failed"));
        let notice = md.find("the agent restarted").unwrap();
        assert!(md.find("### Tool call: solid.box").unwrap() < notice, "the notice keeps its place after the calls");
        assert!(notice < md.find("## Assistant").unwrap(), "and before the reply that followed it");
        assert!(md.contains("sensitive data"), "tool-inclusive export warns about retained content");
    }

    #[test]
    fn unavailable_tool_content_is_reported_not_reconstructed() {
        let md = markdown(&sample_items(), true);
        assert!(md.contains("### Tool call: (untitled)"));
        assert!(md.contains("- Status: not reported"));
        assert!(md.contains("Input: not retained."));
        assert!(md.contains("Output: not retained."));
    }

    #[test]
    fn cards_plans_and_changes_never_export() {
        for tools in [false, true] {
            let md = markdown(&sample_items(), tools);
            assert!(!md.contains("run a command?"));
            assert!(!md.contains("replace the design"));
            assert!(!md.contains("do it"), "plans stay out");
        }
    }

    #[test]
    fn attachment_names_are_listed_without_binaries() {
        let md = markdown(&sample_items(), false);
        assert!(md.contains("Attachments: sample.step, view.png"));
        assert!(!md.contains("\u{1}\u{2}\u{3}"), "no binary bytes");
    }

    #[test]
    fn empty_conversation_still_writes_a_header() {
        let md = markdown(&[], false);
        assert!(md.starts_with("# SolidCraft Assistant conversation"));
        assert!(!md.contains("## User"));
    }

    fn app_with_items(answer: Option<String>) -> (SolveApp, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("sc-10677-{}-{}", std::process::id(), {
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos()
        }));
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.agent_chat.state = Some(State::Ready);
        app.agent_chat.items = sample_items();
        let file = dir.join("out.md");
        let answer = answer.map(|_| file.to_string_lossy().into_owned());
        app.services.pick_save = Some(Box::new(move |name, extensions| {
            assert_eq!((name, extensions), (FILE_NAME, &["md"][..]));
            let answer = answer.clone();
            Box::pin(std::future::ready(answer))
        }));
        (app, file)
    }

    fn finish_picker(app: &mut SolveApp) {
        let ctx = egui::Context::default();
        crate::file_picker::tick(app, &ctx);
    }

    #[test]
    fn export_writes_the_snapshot_and_touches_nothing_else() {
        let (mut app, file) = app_with_items(Some("save".into()));
        let doc_before = app.session.doc.clone();
        let items_before = app.agent_chat.items.len();
        start(&mut app, true);
        finish_picker(&mut app);
        let written = std::fs::read_to_string(&file).unwrap();
        assert!(written.contains("### Tool call: solid.box"));
        assert!(written.contains("## Assistant"));
        assert!(Arc::ptr_eq(&doc_before, &app.session.doc), "the design is untouched");
        assert_eq!(app.agent_chat.items.len(), items_before, "the chat is untouched");
        assert!(app.status.as_ref().is_some_and(|(_, _, error)| !*error), "{:?}", app.status);
        std::fs::remove_dir_all(file.parent().unwrap()).ok();
    }

    #[test]
    fn export_cancel_writes_nothing_and_reports_nothing() {
        let (mut app, file) = app_with_items(None);
        start(&mut app, false);
        finish_picker(&mut app);
        assert!(!file.exists(), "a cancel writes no file");
        assert!(app.status.is_none(), "a cancel reports nothing");
        std::fs::remove_dir_all(file.parent().unwrap()).ok();
    }

    #[test]
    fn export_write_failure_is_reported() {
        let (mut app, file) = app_with_items(Some("save".into()));
        // A directory in the way: the write must fail and say so.
        std::fs::create_dir(&file).unwrap();
        start(&mut app, false);
        finish_picker(&mut app);
        assert!(app.status.as_ref().is_some_and(|(_, _, error)| *error), "{:?}", app.status);
        assert!(app.status.as_ref().unwrap().0.contains("Could not export"), "{:?}", app.status);
        std::fs::remove_dir_all(file.parent().unwrap()).ok();
    }

    #[test]
    fn snapshot_is_stable_while_a_reply_streams() {
        let mut chat = AgentChat::default();
        chat.items.push(Item::Agent { thought: false, text: "partial".into() });
        let md = markdown(&chat.items, false);
        // The reply keeps streaming after the snapshot was taken.
        if let Item::Agent { text, .. } = chat.items.last_mut().unwrap() {
            text.push_str(" and more");
        }
        assert!(md.contains("partial\n") || md.ends_with("partial\n"));
        assert!(!md.contains("and more"), "the captured text does not change with the stream");
    }
}
