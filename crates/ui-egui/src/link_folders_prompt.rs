//! Linked files from other folders load after you allow them. When a design opens, a linked
//! design in its own folder (or below it) whose file changed is read again as it opens. One in
//! another folder is read again too once the user has allowed that folder (Preferences ›
//! General, `linked_file_folders`); otherwise the user is asked once: Load, which reads them now
//! and remembers their folders, or Not now, which leaves them out of date until Update Links.
//!
//! The folders allowed are the user's own settings, never the design's. A design opened through
//! the control channel or by the assistant never asks; its links in other folders stay out of
//! date, allowed folders included (it may work within narrower folders than the user), and
//! `component.update_links` reads them on request.

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
/// (`links`: each link with its folder). Nothing is asked when there are none. Each design keeps
/// its own question: one waiting for a design in another tab is asked when that tab is shown.
pub(crate) fn ask(app: &mut SolveApp, links: Vec<(u64, String)>) {
    if links.is_empty() {
        return;
    }
    let session = app.session.uid;
    let at = match app.link_folders_prompt.iter().position(|p| p.session == session) {
        Some(at) => at,
        None => {
            // Questions for designs closed unanswered are dropped first (as after Not now).
            let open: Vec<u64> = crate::documents::open_sessions(app).iter().map(|s| s.uid).collect();
            app.link_folders_prompt.retain(|p| open.contains(&p.session));
            app.link_folders_held.retain(|(s, _)| open.contains(s));
            app.link_folders_prompt.push(LinkFoldersPrompt { session, folders: Vec::new(), components: Vec::new() });
            app.link_folders_prompt.len() - 1
        }
    };
    let Some(prompt) = app.link_folders_prompt.get_mut(at) else { return };
    for (component, folder) in links {
        if !prompt.components.contains(&component) {
            prompt.components.push(component);
        }
        if !prompt.folders.contains(&folder) {
            prompt.folders.push(folder);
        }
    }
}

/// Whether the shown design's link `component` is held back: Not now was answered for it, or its
/// question is still open.
pub(crate) fn held(app: &SolveApp, component: u64) -> bool {
    let session = app.session.uid;
    app.link_folders_held.contains(&(session, component))
        || app.link_folders_prompt.iter().any(|p| p.session == session && p.components.contains(&component))
}

/// Act on the answer to the shown design's question. Load remembers the folders and reads the links.
pub fn answer(app: &mut SolveApp, answer: Answer) {
    let shown = app.session.uid;
    let Some(at) = app.link_folders_prompt.iter().position(|p| p.session == shown) else { return };
    let LinkFoldersPrompt { session, folders, components } = app.link_folders_prompt.remove(at);
    if answer == Answer::NotNow {
        app.link_folders_held.extend(components.into_iter().map(|c| (session, c)));
        return;
    }
    app.link_folders_held.retain(|(s, c)| *s != session || !components.contains(c));
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
    let Some(prompt) = app.link_folders_prompt.iter().find(|p| p.session == app.session.uid) else { return };
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    let folders = prompt.folders.join(", ");
    let detail = tr!("linked_files.other_folders_detail", folders = &folders);
    let (closed, action) = ui_kit::Confirm::new(egui::Id::unique("sc_link_folders_prompt"), "help", tr!("linked_files.other_folders_title"))
        .actions(tr!("linked_files.other_folders_load"), tr!("linked_files.other_folders_not_now"), None)
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
