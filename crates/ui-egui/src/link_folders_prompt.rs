//! Linked files from other folders load after you allow them. When a design opens, a linked
//! design in its own folder (or below it) whose file changed is read again as it opens. One in
//! another folder is read again too once the user has allowed that folder (Preferences ›
//! General, `linked_file_folders`); otherwise the user is asked once: Load, which reads them now
//! and remembers their folders, or Not now, which leaves them out of date until Update Links.
//!
//! The folders allowed are the user's own settings, never the design's. A design opened through
//! the control channel or by the assistant never asks; its links in other folders stay out of
//! date unless their folder was allowed, and `component.update_links` reads them on request.

use crate::SolveApp;
use crate::ui_kit;

/// The question waiting for its answer.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkFoldersPrompt {
    /// The design it is about ([`solidcraft_engine::Session::uid`]).
    pub session: u64,
    /// The folders, as they are shown and remembered, each once.
    pub folders: Vec<String>,
    /// The links in them that are out of date.
    pub components: Vec<u64>,
}

/// The answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Load,
    NotNow,
}

/// Ask about the out-of-date links of the shown design that are in folders not allowed yet
/// (`links`: each link with its folder). Nothing is asked when there are none. A question still
/// waiting for another design is dropped: that one's links stay out of date, as after Not now.
pub(crate) fn ask(app: &mut SolveApp, links: Vec<(u64, String)>) {
    if links.is_empty() {
        return;
    }
    let session = app.session.uid;
    if app.link_folders_prompt.as_ref().is_some_and(|p| p.session != session) {
        app.link_folders_prompt = None;
    }
    let prompt = app.link_folders_prompt.get_or_insert_with(|| LinkFoldersPrompt { session, folders: Vec::new(), components: Vec::new() });
    for (component, folder) in links {
        if !prompt.components.contains(&component) {
            prompt.components.push(component);
        }
        if !prompt.folders.contains(&folder) {
            prompt.folders.push(folder);
        }
    }
}

/// Act on the answer. Load remembers the folders and reads the links, if their design is still
/// the one shown (otherwise Update Links reads them there).
pub fn answer(app: &mut SolveApp, answer: Answer) {
    let Some(LinkFoldersPrompt { session, folders, components }) = app.link_folders_prompt.take() else { return };
    if answer == Answer::NotNow {
        return;
    }
    for folder in folders {
        if !app.preferences.linked_file_folders.contains(&folder) {
            app.preferences.linked_file_folders.push(folder);
        }
    }
    if session == app.session.uid {
        crate::linked_source::read(app, components);
    }
}

/// The question, when one is waiting for the design shown (one for a design in another tab waits
/// for its tab to come back).
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(prompt) = app.link_folders_prompt.as_ref().filter(|p| p.session == app.session.uid) else { return };
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    let folders = prompt.folders.join(", ");
    let detail = solidcraft_ui_text::format("linked_files.other_folders_detail", &[("folders", &folders)]);
    let (closed, action) =
        ui_kit::Confirm::new(egui::Id::unique("sc_link_folders_prompt"), "help", solidcraft_ui_text::tr("linked_files.other_folders_title"))
            .actions(solidcraft_ui_text::tr("linked_files.other_folders_load"), solidcraft_ui_text::tr("linked_files.other_folders_not_now"), None)
            .selected(ui_kit::Action::Cancel)
            .show(ctx, true, |ui| {
                ui_kit::text(ui, &detail, ui_kit::Text::Report("link_folders_prompt", false));
            });
    let mut chosen = match action {
        ui_kit::Action::Ok => Some(Answer::Load),
        ui_kit::Action::Cancel => Some(Answer::NotNow),
        _ if closed => Some(Answer::NotNow),
        _ => None,
    };
    if ctx.input(|x| x.key_pressed(egui::Key::Escape)) {
        chosen = Some(Answer::NotNow);
        app.esc_handled = true;
    }
    if let Some(a) = chosen {
        answer(app, a);
    }
}
