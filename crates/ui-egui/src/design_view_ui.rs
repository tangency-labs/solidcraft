//! What the user does with a design's views from the tab strip: the menus' rows, the handler for
//! the strip's view actions, and the notices a sheet view shows. The views themselves are
//! [`crate::design_views`]; this file only maps a chosen row to a `documents::view_*` call.
//!
//! Plug-in point for sheets: when a sheet view exists, call [`deleted_sheet_notice`] while its
//! sheet is gone and [`stale_source_notice`] while its design has unsaved changes, draw the result
//! with [`show_notice`], and act on the [`NoticeChoice`] it returns.

use std::path::Path;

use crate::app_bar::{TabAction, TabMenuRow};
use crate::design_views::ViewKind;
use crate::{SolveApp, documents, new_window, ui_kit};

/// Menu row ids of a view's right-click menu ([`VIEW_MENU`]).
pub const VIEW_RENAME: &str = "view.rename";
pub const VIEW_DUPLICATE: &str = "view.duplicate";
pub const VIEW_NEW_WINDOW: &str = "view.new_window";
pub const VIEW_CLOSE: &str = "view.close";
pub const VIEW_CLOSE_OTHERS: &str = "view.close_others";
pub const VIEW_CLOSE_RIGHT: &str = "view.close_right";
pub const DESIGN_CLOSE: &str = "design.close";

/// Menu row ids of the "+" menu ([`plus_menu`]); a row that opens a kind is `plus.` and its key.
pub const PLUS_DUPLICATE: &str = "plus.duplicate";
pub const PLUS_REOPEN: &str = "plus.reopen";

/// Menu row ids of the view switcher's footer.
pub const SWITCHER_OPEN: &str = "switcher.open";
pub const SWITCHER_REOPEN: &str = "switcher.reopen";
pub const SWITCHER_AS_TABS: &str = "switcher.as_tabs";

/// A view's right-click menu in three groups set apart by lines: rename, copy and move; close
/// views; close the design.
pub const VIEW_MENU: [TabMenuRow<'static>; 7] = [
    TabMenuRow::new(VIEW_RENAME, "Rename view", "F2").with_icon("rename", None),
    TabMenuRow::new(VIEW_DUPLICATE, "Duplicate view", "").with_icon("duplicate", None),
    TabMenuRow::new(VIEW_NEW_WINDOW, "Open in new window", "").with_icon("window", None),
    TabMenuRow::new(VIEW_CLOSE, "Close view", "Ctrl+W").with_icon("close", None).separated(),
    TabMenuRow::new(VIEW_CLOSE_OTHERS, "Close other views", "").needing_others(),
    TabMenuRow::new(VIEW_CLOSE_RIGHT, "Close views to the right", "").needing_others(),
    TabMenuRow::new(DESIGN_CLOSE, "Close design", "Ctrl+Shift+W").with_icon("close", None).separated(),
];

/// The "+" menu: a header, one row per view kind with its icon in the kind's accent (`dark` picks
/// the accent for the menu's ground), then copy the shown view and bring back the last closed one.
pub fn plus_menu(dark: bool) -> [TabMenuRow<'static>; 8] {
    let kind = |id, label: &'static str, icon| TabMenuRow::new(id, label, "").with_icon(icon, Some(crate::workspace::view_icon_colour(label, dark)));
    [
        TabMenuRow::heading("Open as a view"),
        kind("plus.model", "Model", "model"),
        kind("plus.sketch", "Sketch", "sketch"),
        kind("plus.sheet", "Sheet", "sheet"),
        kind("plus.render", "Render", "render"),
        kind("plus.animation", "Animation", "animation"),
        TabMenuRow::new(PLUS_DUPLICATE, "Duplicate this view", "").with_icon("duplicate", None).separated(),
        TabMenuRow::new(PLUS_REOPEN, "Reopen closed view", "Ctrl+Shift+T").with_icon("reopen", None),
    ]
}

/// The id of the shown design's view at place `index`.
fn view_id(app: &SolveApp, index: usize) -> Option<u64> {
    app.docs.views.views().get(index).map(|view| view.id)
}

/// Carry out one of the strip's view actions. False for an action that is not a view action.
pub fn handle(app: &mut SolveApp, action: TabAction) -> bool {
    match action {
        TabAction::SelectView(i) => {
            if let Some(id) = view_id(app, i) {
                documents::view_activate(app, id);
            }
        }
        TabAction::CloseView(i) => {
            if let Some(id) = view_id(app, i) {
                documents::view_close(app, id);
            }
        }
        TabAction::NewView => {
            documents::view_open(app, ViewKind::Model, None, false);
        }
        TabAction::ReorderView { from, to } => documents::view_move(app, from, to),
        TabAction::ViewMenu { view, id } => return view_menu(app, view, &id),
        TabAction::PlusMenu { id } => return plus_row(app, &id),
        TabAction::SwitcherMenu { id } => return switcher_menu(app, &id),
        TabAction::TearOff(i) => {
            documents::switch(app, i);
            open_new_window(app, None);
        }
        _ => return false,
    }
    true
}

fn view_menu(app: &mut SolveApp, index: usize, id: &str) -> bool {
    let Some(view) = view_id(app, index) else { return false };
    match id {
        VIEW_RENAME => {
            documents::view_rename_start(app, view);
        }
        VIEW_DUPLICATE => {
            documents::view_duplicate(app, view);
        }
        VIEW_NEW_WINDOW => {
            let kind = app.docs.views.get(view).map(|v| v.kind.key());
            open_new_window(app, kind);
        }
        VIEW_CLOSE => {
            documents::view_close(app, view);
        }
        VIEW_CLOSE_OTHERS => documents::view_close_others(app, view),
        VIEW_CLOSE_RIGHT => documents::view_close_right_of(app, view),
        DESIGN_CLOSE => documents::close(app, app.docs.active, false),
        _ => return false,
    }
    true
}

fn plus_row(app: &mut SolveApp, id: &str) -> bool {
    if let Some(kind) = id.strip_prefix("plus.").and_then(ViewKind::from_key) {
        let reuse = matches!(kind, ViewKind::Model | ViewKind::Render) && app.preferences.design_reuse_views;
        documents::view_open(app, kind, None, reuse);
        return true;
    }
    match id {
        PLUS_DUPLICATE => {
            let active = app.docs.views.active().id;
            documents::view_duplicate(app, active);
        }
        PLUS_REOPEN => {
            documents::view_reopen(app);
        }
        _ => return false,
    }
    true
}

fn switcher_menu(app: &mut SolveApp, id: &str) -> bool {
    match id {
        SWITCHER_OPEN => {
            documents::view_open(app, ViewKind::Model, None, false);
        }
        SWITCHER_REOPEN => {
            documents::view_reopen(app);
        }
        SWITCHER_AS_TABS => app.preferences.design_views_as_tabs = true,
        _ => return false,
    }
    true
}

/// Move the shown design to a new window (on the view named `view`, a [`ViewKind::key`]). A design
/// that is not saved says so and stays; one that starts the new window leaves this one.
fn open_new_window(app: &mut SolveApp, view: Option<&str>) {
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    let name = crate::i18n::tr_name(&documents::tab_name(app, &app.session));
    let path = app.session.path.clone().filter(|_| !new_window::needs_save_first(app.session.path.as_deref(), app.session.is_dirty()));
    let Some(path) = path else {
        app.set_status(solidcraft_ui_text::format("ui.new_window.save_first", &[("name", &name)]), false);
        return;
    };
    match new_window::open_in_new_window(Path::new(&path), view) {
        Ok(()) => {
            documents::close(app, app.docs.active, true);
            app.set_status(solidcraft_ui_text::format("ui.new_window.opened", &[("name", &name)]), false);
        }
        Err(error) => {
            let reason = if error == "ui.new_window.unsupported" { solidcraft_ui_text::tr(&error).to_string() } else { error };
            app.set_status(solidcraft_ui_text::format("ui.new_window.failed", &[("error", &reason)]), true);
        }
    }
}

/// The line "Closing the design closes its N views. Closing a single view never asks." the close
/// prompt shows for a design with several views; empty for a design with one.
pub fn closing_detail(views: usize) -> String {
    if views < 2 {
        return String::new();
    }
    solidcraft_ui_text::format("design_view.close_design_detail", &[("n", &views.to_string())])
}

/// A band over a view saying something the user may act on, in the words shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    /// A second line; empty when the title says it all.
    pub body: String,
    pub primary: String,
    pub secondary: String,
}

/// Which button of a [`Notice`] was pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeChoice {
    Primary,
    Secondary,
}

/// The notice for a sheet view whose sheet `sheet` was deleted: Undo delete (primary) brings it
/// back, Close view (secondary) lets the view go.
pub fn deleted_sheet_notice(sheet: &str) -> Notice {
    Notice {
        title: solidcraft_ui_text::format("design_view.sheet_deleted", &[("name", sheet)]),
        body: solidcraft_ui_text::tr("design_view.sheet_deleted_body").to_string(),
        primary: solidcraft_ui_text::tr("design_view.undo_delete").to_string(),
        secondary: solidcraft_ui_text::tr("design_view.close_view").to_string(),
    }
}

/// The notice for a sheet view whose design `design` has unsaved changes the sheet does not show:
/// Save (primary) writes them, Follow live (secondary) makes the sheet show them.
pub fn stale_source_notice(design: &str) -> Notice {
    Notice {
        title: solidcraft_ui_text::format("design_view.source_unsaved", &[("name", design)]),
        body: String::new(),
        primary: solidcraft_ui_text::format("design_view.save_design", &[("name", design)]),
        secondary: solidcraft_ui_text::tr("design_view.follow_live").to_string(),
    }
}

/// Draw `notice` as a warning band with its two buttons; the button pressed this frame.
pub fn show_notice(ui: &mut egui::Ui, notice: &Notice) -> Option<NoticeChoice> {
    let t = solidcraft_theme::Tokens::get();
    ui_kit::banner(ui, ui_kit::AssistantTone::Warning, |ui| {
        ui.add(egui::Label::new(egui::RichText::new(&notice.title).color(t.form_ink)).wrap());
        if !notice.body.is_empty() {
            ui.add(egui::Label::new(egui::RichText::new(&notice.body).size(t.font_label).color(t.form_dim)).wrap());
        }
        let mut chosen = None;
        ui.horizontal(|ui| {
            if ui_kit::button(ui, notice.primary.as_str(), ui_kit::Button::Small).clicked() {
                chosen = Some(NoticeChoice::Primary);
            }
            if ui_kit::button(ui, notice.secondary.as_str(), ui_kit::Button::Small).clicked() {
                chosen = Some(NoticeChoice::Secondary);
            }
        });
        chosen
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_are_unique_and_plus_rows_name_every_kind() {
        let plus = plus_menu(false);
        let ids: Vec<&str> = VIEW_MENU.iter().chain(plus.iter()).map(|r| r.id).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len());
        for kind in ViewKind::ALL {
            assert!(plus.iter().any(|r| r.id.strip_prefix("plus.") == Some(kind.key()) && r.icon == kind.key() && r.accent.is_some()));
        }
        assert!(plus[0].heading && plus.iter().skip(1).all(|r| !r.heading));
    }

    #[test]
    fn view_menu_groups_are_set_apart_by_lines() {
        let starts: Vec<&str> = VIEW_MENU.iter().filter(|r| r.separator_before).map(|r| r.id).collect();
        assert_eq!(starts, [VIEW_CLOSE, DESIGN_CLOSE]);
    }

    #[test]
    fn close_design_detail_only_for_several_views() {
        assert_eq!(closing_detail(1), "");
        assert!(!closing_detail(4).is_empty());
    }

    #[test]
    fn notices_carry_their_title_body_and_two_choices() {
        for n in [deleted_sheet_notice("Sheet 2"), stale_source_notice("Bracket v3")] {
            assert!(!n.title.is_empty());
            assert!(!n.primary.is_empty() && !n.secondary.is_empty() && n.primary != n.secondary);
        }
    }
}
