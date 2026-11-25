//! The Notes panel (`ui.notes`, Ctrl+Shift+N): write a note on what is selected, see the
//! design's notes, tick them off, and copy the open ones for an agent to work off.
//!
//! The panel is thin: every change runs a note command (`note.add`, `note.update`,
//! `note.delete`), so notes are saved and undone like any other design change, and an agent
//! reads the same notes with `note.list` over MCP.

use egui::RichText;
use serde_json::{Value, json};
use solidcraft_ui_text::{format as trf, tr};

use crate::SolveApp;

/// What the panel keeps between frames: the new note's text, the note being edited and the
/// filter. Drafts belong to the design they were written in.
#[derive(Default)]
pub(crate) struct State {
    document: String,
    draft: String,
    focus_draft: bool,
    editing: Option<(u64, String)>,
    show_done: bool,
    /// Text the header action copies on the next frame (it runs without the egui context).
    copy_pending: Option<String>,
}

/// Show or hide the panel (`ui.notes`).
pub(crate) fn toggle(app: &mut SolveApp) {
    if let Err(reason) = crate::panel_runtime::toggle_notes(app, false) {
        app.set_status(solidcraft_engine::panels::commands::error_text(reason), true);
    }
}

/// Show the panel with the new note's text field focused: Add Note started by the user.
pub(crate) fn open_for_new_note(app: &mut SolveApp) {
    app.notes.focus_draft = true;
    if let Err(reason) = crate::panel_runtime::toggle_notes(app, true) {
        app.set_status(solidcraft_engine::panels::commands::error_text(reason), true);
    }
}

/// The open notes as text for an agent, copied to the clipboard (the panel's header action).
pub(crate) fn copy_for_agent(app: &mut SolveApp) {
    let text = agent_text(app);
    app.notes.copy_pending = Some(text);
    app.set_status(tr("ui.notes_copied"), false);
}

/// The panel's content.
pub(crate) fn shared_content(app: &mut SolveApp, ui: &mut egui::Ui) {
    if app.notes.document != app.session.document_id {
        // Requests made before the first frame (Add Note focusing the draft, a copy) survive.
        let copy_pending = app.notes.copy_pending.take();
        let focus_draft = app.notes.focus_draft;
        app.notes = State { document: app.session.document_id.clone(), copy_pending, focus_draft, ..State::default() };
    }
    if let Some(text) = app.notes.copy_pending.take() {
        ui.ctx().copy_text(text);
    }
    let top = ui.cursor().top();
    let t = solidcraft_theme::Tokens::get();

    new_note(app, ui, &t);
    ui.add_space(t.space_6);
    ui.separator();

    let notes = app.session.doc.notes.clone();
    let open = notes.iter().filter(|note| !note.done).count();
    ui.horizontal(|ui| {
        ui.label(RichText::new(trf("ui.notes_open_count", &[("open", &open.to_string()), ("all", &notes.len().to_string())])).color(t.text_dim));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut show_done = app.notes.show_done;
            let response = crate::ui_kit::checkbox(ui, tr("ui.notes_show_done"), &mut show_done);
            crate::panel_runtime::register_content_focus(ui.ctx(), &response);
            app.notes.show_done = show_done;
        });
    });

    let shown: Vec<_> = notes.iter().filter(|note| app.notes.show_done || !note.done).collect();
    if shown.is_empty() {
        ui.add_space(t.space_8);
        ui.label(RichText::new(tr(if notes.is_empty() { "ui.notes_empty" } else { "ui.notes_all_done" })).color(t.text_dim));
    }
    let max = ui.available_height().max(t.control_height * 3.0);
    let scroll = egui::ScrollArea::vertical().id_salt("notes-list").min_scrolled_height(0.0).max_height(max).show(ui, |ui| {
        for note in shown {
            note_row(app, ui, &t, note);
        }
    });

    ui.add_space(t.space_6);
    let copy = crate::ui_kit::button(ui, tr("ui.notes_copy_for_agent"), crate::ui_kit::Button::Enabled(open > 0));
    crate::panel_runtime::register_content_focus(ui.ctx(), &copy);
    if copy.clicked() {
        ui.ctx().copy_text(agent_text(app));
        app.set_status(tr("ui.notes_copied"), false);
    }
    let natural = (ui.cursor().top() - top) + scroll.content_size.y - scroll.inner_rect.height();
    crate::panel_runtime::report_content_extent(ui, top, natural.max(t.control_height), false);
}

/// The text field and button that make a note on the selection.
fn new_note(app: &mut SolveApp, ui: &mut egui::Ui, t: &solidcraft_theme::Tokens) {
    let target = selection_target(app);
    let on = match &target {
        Some(target) => trf("ui.notes_new_on", &[("target", target)]),
        None => tr("ui.notes_new_nothing_selected").to_string(),
    };
    ui.label(RichText::new(on).color(t.text_dim).size(t.font_caption));
    let id = egui::Id::unique("notes-panel-draft");
    let mut draft = std::mem::take(&mut app.notes.draft);
    let field =
        ui.add(egui::TextEdit::multiline(&mut draft).id(id).hint_text(tr("ui.notes_draft_hint")).desired_rows(3).desired_width(ui.available_width()));
    crate::panel_runtime::register_content_focus(ui.ctx(), &field);
    if std::mem::take(&mut app.notes.focus_draft) {
        field.request_focus();
    }
    let submit_key = field.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
    let add = crate::ui_kit::button(ui, tr("ui.notes_add"), crate::ui_kit::Button::Enabled(!draft.trim().is_empty()));
    crate::panel_runtime::register_content_focus(ui.ctx(), &add);
    if (add.clicked() || submit_key) && !draft.trim().is_empty() {
        match app.run("note.add", json!({ "text": draft.trim() })) {
            Ok(_) => draft.clear(),
            Err(e) => app.set_status(e, true),
        }
    }
    app.notes.draft = draft;
}

/// One note: done box, text, what it is on, and its actions.
fn note_row(app: &mut SolveApp, ui: &mut egui::Ui, t: &solidcraft_theme::Tokens, note: &solidcraft_engine::doc::Note) {
    ui.add_space(t.space_4);
    let editing = app.notes.editing.as_ref().is_some_and(|(id, _)| *id == note.id);
    ui.horizontal_top(|ui| {
        let mut done = note.done;
        let check = crate::ui_kit::checkbox(ui, "", &mut done);
        crate::panel_runtime::register_content_focus(ui.ctx(), &check);
        if done != note.done {
            run(app, "note.update", json!({ "note": note.id, "done": done }));
        }
        ui.vertical(|ui| {
            if editing {
                edit_row(app, ui, note.id);
            } else {
                let mut text = RichText::new(&note.text).color(if note.done { t.text_dim } else { t.text });
                if note.done {
                    text = text.strikethrough();
                }
                ui.add(egui::Label::new(text).wrap());
            }
            ui.label(RichText::new(where_text(note)).color(t.text_dim).size(t.font_caption));
            if !editing {
                ui.horizontal_wrapped(|ui| {
                    let show = crate::ui_kit::button(ui, tr("ui.notes_show"), crate::ui_kit::Button::Small);
                    crate::panel_runtime::register_content_focus(ui.ctx(), &show);
                    if show.clicked() {
                        show_note(app, note);
                    }
                    let edit = crate::ui_kit::button(ui, tr("ui.notes_edit"), crate::ui_kit::Button::Small);
                    crate::panel_runtime::register_content_focus(ui.ctx(), &edit);
                    if edit.clicked() {
                        app.notes.editing = Some((note.id, note.text.clone()));
                    }
                    let hide_key = if note.hidden { "ui.notes_unhide" } else { "ui.notes_hide" };
                    let hide = crate::ui_kit::button(ui, tr(hide_key), crate::ui_kit::Button::Small);
                    crate::panel_runtime::register_content_focus(ui.ctx(), &hide);
                    if hide.clicked() {
                        run(app, "note.update", json!({ "note": note.id, "hidden": !note.hidden }));
                    }
                    let delete = crate::ui_kit::button(ui, tr("ui.notes_delete"), crate::ui_kit::Button::Small);
                    crate::panel_runtime::register_content_focus(ui.ctx(), &delete);
                    if delete.clicked() {
                        run(app, "note.delete", json!({ "note": note.id }));
                    }
                });
            }
        });
    });
    ui.separator();
}

/// The text field of the note being edited, with Save and Cancel.
fn edit_row(app: &mut SolveApp, ui: &mut egui::Ui, id: u64) {
    let Some((_, mut text)) = app.notes.editing.take() else { return };
    let field = ui.add(egui::TextEdit::multiline(&mut text).desired_rows(3).desired_width(ui.available_width()));
    crate::panel_runtime::register_content_focus(ui.ctx(), &field);
    let mut keep = true;
    ui.horizontal(|ui| {
        let save = crate::ui_kit::button(ui, tr("ui.notes_save"), crate::ui_kit::Button::Enabled(!text.trim().is_empty()));
        crate::panel_runtime::register_content_focus(ui.ctx(), &save);
        if save.clicked() && !text.trim().is_empty() {
            keep = !run(app, "note.update", json!({ "note": id, "text": text.trim() }));
        }
        let cancel = crate::ui_kit::button(ui, tr("ui.notes_cancel"), crate::ui_kit::Button::Small);
        crate::panel_runtime::register_content_focus(ui.ctx(), &cancel);
        if cancel.clicked() {
            keep = false;
        }
    });
    if keep {
        app.notes.editing = Some((id, text));
    }
}

/// Run a note command; a refusal shows in the status bar. True when it ran.
fn run(app: &mut SolveApp, id: &str, params: Value) -> bool {
    match app.run(id, params) {
        Ok(_) => true,
        Err(e) => {
            app.set_status(e, true);
            false
        }
    }
}

/// Select what the note is on, so it lights up in the viewport.
fn show_note(app: &mut SolveApp, note: &solidcraft_engine::doc::Note) {
    let point = [note.anchor.x, note.anchor.y, note.anchor.z];
    let item = match &note.target {
        Some(target) if target.kind == "body" => json!({ "type": "body", "name": target.body }),
        Some(target) => json!({ "type": target.kind, "body": target.body, "point": point }),
        None => {
            app.set_status(tr("ui.notes_no_target"), false);
            return;
        }
    };
    run(app, "select.set", json!({ "items": [item] }));
}

/// What a note is on and where, for the list and for an agent.
fn where_text(note: &solidcraft_engine::doc::Note) -> String {
    let at = format!("({:.1}, {:.1}, {:.1}) mm", note.anchor.x, note.anchor.y, note.anchor.z);
    match &note.target {
        Some(target) => trf("ui.notes_where_target", &[("target", &target_text(&target.kind, &target.body, target.index)), ("at", &at)]),
        None => trf("ui.notes_where_point", &[("at", &at)]),
    }
}

fn target_text(kind: &str, body: &str, index: Option<usize>) -> String {
    let key = match kind {
        "face" => "ui.notes_target_face",
        "edge" => "ui.notes_target_edge",
        "vertex" => "ui.notes_target_vertex",
        _ => "ui.notes_target_body",
    };
    trf(key, &[("body", body), ("index", &index.map_or_else(String::new, |i| i.to_string()))])
}

/// What a note made now would be on: the item the engine takes the anchor from.
fn selection_target(app: &SolveApp) -> Option<String> {
    // The engine's rule, so the label names what `note.add` will save as the target.
    solidcraft_engine::cmd::note_target_from_selection(&app.session).map(|t| target_text(&t.kind, &t.body, t.index))
}

/// The open notes, numbered, with what each is on, the design and how to tick them off.
fn agent_text(app: &SolveApp) -> String {
    let mut out = trf("ui.notes_agent_header", &[("design", &app.session.doc.name)]);
    out.push('\n');
    if let Some(path) = app.session.path.as_ref() {
        out.push_str(path);
        out.push('\n');
    }
    out.push('\n');
    for (n, note) in app.session.doc.notes.iter().filter(|note| !note.done).enumerate() {
        out.push_str(&format!("{}. [note {}] {}\n", n + 1, note.id, where_text(note)));
        for line in note.text.lines() {
            out.push_str(&format!("   {line}\n"));
        }
    }
    out.push('\n');
    out.push_str(tr("ui.notes_agent_footer"));
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_agent_text_lists_open_notes_with_their_target() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length": 10, "width": 20, "height": 30, "body_name": "Box"})).unwrap();
        h.app.run("select.set", json!({"items": [{"type": "face", "body": "Box", "point": [5.0, 10.0, 30.0]}]})).unwrap();
        h.app.run("note.add", json!({"text": "Bore this to 8.5"})).unwrap();
        let done = h.app.run("note.add", json!({"anchor": [0.0, 0.0, 0.0], "text": "Already handled"})).unwrap()["note"].as_u64().unwrap();
        h.app.run("note.update", json!({"note": done, "done": true})).unwrap();
        let text = agent_text(&h.app);
        assert!(text.contains("Bore this to 8.5"));
        assert!(text.contains("Box"));
        assert!(!text.contains("Already handled"));
    }
}
