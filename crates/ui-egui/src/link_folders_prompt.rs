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

/// The folder list's heights: (its first height, the most it can be dragged to, the least). It
/// starts at about eight lines, less for a few folders or in a short window, and its corner can
/// drag it as tall as the window allows: the title and the Load and Not now buttons stay on
/// screen however many folders there are.
fn list_heights(ctx: &egui::Context, folders: usize) -> (f32, f32, f32) {
    let t = solidcraft_theme::Tokens::get();
    let line = t.font_small + t.space_6;
    let chrome = t.form_close_size + t.form_footer_height + t.space_24 * 4.0 + t.font_subtitle * 2.0;
    let least = line * 2.0;
    let most = (ctx.content_rect().height() - chrome).max(least);
    // A long folder wraps to a few lines in the dialog's width.
    let wanted = folders as f32 * line * 3.0;
    ((line * 8.0).min(wanted).clamp(least, most), most, least)
}

/// The question, when one is waiting for the design shown (one for a design in another tab waits
/// for its tab to come back).
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(prompt) = app.link_folders_prompt.iter().find(|p| p.session == app.session.uid) else { return };
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    let detail = solidcraft_ui_text::tr("linked_files.other_folders_detail");
    let (height, most, least) = list_heights(ctx, prompt.folders.len());
    let card = solidcraft_theme::Tokens::get().form_card_width;
    let usual = card * 1.5;
    let screen = ctx.content_rect().width();
    // What the dialog adds around the list's width, as last laid out (a dialog of the usual width
    // around a list that fits it, before the first frame).
    let around_id = egui::Id::unique("sc_link_folders_prompt_around");
    let around = ctx.data(|d| d.get_temp::<f32>(around_id)).unwrap_or(0.0);
    // The list's size once its corner was dragged, kept while the app runs; until then each
    // question starts at the size its folders and the window give. The drag is applied before
    // anything is laid out, so the dialog widens in the same frame as the list.
    let size_id = egui::Id::unique("sc_link_folders_prompt_size");
    let grip_id = egui::Id::unique("sc_link_folders_prompt_grip");
    let mut dragged = ctx.data(|d| d.get_temp::<egui::Vec2>(size_id));
    if ctx.is_being_dragged(grip_id) {
        let delta = ctx.input(|i| i.pointer.delta());
        let from = dragged.unwrap_or(egui::vec2(usual - around, height));
        dragged = Some(from + delta);
    }
    let width_range = (usual - around).max(0.0)..=(screen - around).max(usual - around).max(0.0);
    let dragged = dragged.map(|s| egui::vec2(s.x.clamp(*width_range.start(), *width_range.end()), s.y.clamp(least, most)));
    if let Some(s) = dragged {
        ctx.data_mut(|d| d.insert_temp(size_id, s));
    }
    // The dialog is as wide as the list: never narrower than a dialog's usual width, nor wider
    // than the window.
    let width = dragged.map_or(usual, |s| (s.x + around).clamp(usual, screen.max(usual)));
    let mut confirm =
        ui_kit::Confirm::new(egui::Id::unique("sc_link_folders_prompt"), "help", solidcraft_ui_text::tr("linked_files.other_folders_title"))
            .actions(solidcraft_ui_text::tr("linked_files.other_folders_load"), solidcraft_ui_text::tr("linked_files.other_folders_not_now"), None)
            .selected(ui_kit::Action::Cancel);
    confirm.window = confirm.window.cards(width / card);
    let (closed, action) = confirm.show(ctx, true, |ui| {
        ui_kit::text(ui, detail, ui_kit::Text::Report("link_folders_prompt", false));
        // One folder a line, in a list that scrolls: an assembly whose parts each have their own
        // folder names dozens, and the buttons below must stay on screen. Its corner drags it
        // taller, to see more folders at once, or wider, and the dialog widens with it.
        ui_kit::list_frame(ui, "link_folders", |ui| {
            let fits = ui.available_width();
            ui.ctx().data_mut(|d| d.insert_temp(around_id, width - fits));
            let list_height = dragged.map_or(height, |s| s.y);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(fits, list_height), egui::Sense::hover());
            let mut list = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
            egui::ScrollArea::vertical().id_salt("link_folders").auto_shrink([false, false]).max_height(list_height).show(&mut list, |ui| {
                for folder in &prompt.folders {
                    ui_kit::text(ui, folder, ui_kit::Text::MonoNote("link_folder"));
                }
            });
            // The corner grip, over the list's bottom right.
            let corner = ui.visuals().resize_corner_size;
            let grip = egui::Rect::from_min_max(rect.right_bottom() - egui::vec2(corner, corner), rect.right_bottom());
            let response = ui.interact(grip, grip_id, egui::Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeNwSe);
            if response.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
            }
            let t = solidcraft_theme::Tokens::get();
            let ink = if response.hovered() || response.dragged() { t.form_ink } else { t.form_dim };
            let stroke = egui::Stroke::new(1.0, ink);
            let painter = ui.painter_at(grip);
            for step in [0.25, 0.5, 0.75] {
                let w = corner * step;
                painter.line_segment([grip.right_bottom() - egui::vec2(w, 0.0), grip.right_bottom() - egui::vec2(0.0, w)], stroke);
            }
            crate::scenario::publish_target("dialog:resize:link_folders", grip);
        });
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

#[cfg(test)]
#[path = "link_folders_prompt_tests.rs"]
mod tests;
