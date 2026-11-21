//! What the user does with a design's views from the tab strip: the menus' rows, the handler for
//! the strip's view actions, and the notices a sheet view shows. The views themselves are
//! [`solidcraft_views`]; this file only maps a chosen row to a `documents::view_*` call.
//!
//! Plug-in point for sheets: when a sheet view exists, call [`deleted_sheet_notice`] while its
//! sheet is gone and [`stale_source_notice`] while its design has unsaved changes, draw the result
//! with [`show_notice`], and act on the [`NoticeChoice`] it returns.

use std::path::Path;

use crate::app_bar::{TabAction, TabMenuRow};
use crate::{SolveApp, documents, new_window, ui_kit};
use solidcraft_views::ViewKind;

/// Menu row ids of a view's right-click menu ([`VIEW_MENU`]).
pub const VIEW_RENAME: &str = "view.rename";
pub const VIEW_DUPLICATE: &str = "view.duplicate";
pub const VIEW_BESIDE: &str = "view.beside";
pub const VIEW_NEW_WINDOW: &str = "view.new_window";
pub const VIEW_MOVE_TO_WINDOW: &str = "view.move_to_window";
const WINDOW_PREFIX: &str = "view.window.";
pub const VIEW_CLOSE: &str = "view.close";
pub const VIEW_CLOSE_OTHERS: &str = "view.close_others";
pub const VIEW_CLOSE_RIGHT: &str = "view.close_right";
pub const DESIGN_CLOSE: &str = "design.close";

/// Menu row ids of the "+" menu ([`plus_menu`]); a row that opens a kind is `plus.` and its key.
pub const PLUS_DUPLICATE: &str = "plus.duplicate";
pub const PLUS_BESIDE: &str = "plus.beside";
/// A layout row is `layout.` and the preset's key ([`solidcraft_views::Preset::key`]).
const LAYOUT_PREFIX: &str = "layout.";
pub const PLUS_REOPEN: &str = "plus.reopen";

/// Menu row ids of the view switcher's footer.
pub const SWITCHER_OPEN: &str = "switcher.open";
pub const SWITCHER_REOPEN: &str = "switcher.reopen";
pub const SWITCHER_AS_TABS: &str = "switcher.as_tabs";

pub const MOVE_TO_WINDOW_MENU: TabMenuRow<'static> = TabMenuRow::new(VIEW_MOVE_TO_WINDOW, "multi_window.move_to_window", "")
    .with_icon("window", None)
    .for_local_view()
    .with_children(&[TabMenuRow::new(VIEW_NEW_WINDOW, "multi_window.new_window", "").with_icon("window", None)]);

/// A view's right-click menu in three groups set apart by lines: rename, copy and move; close
/// views; close the design.
pub const VIEW_MENU: [TabMenuRow<'static>; 9] = [
    TabMenuRow::new(VIEW_RENAME, "Rename view", "F2").with_icon("rename", None).for_local_view(),
    TabMenuRow::new(VIEW_DUPLICATE, "Duplicate view", "").with_icon("duplicate", None).for_local_view(),
    TabMenuRow::new(VIEW_BESIDE, "Show beside", "").with_icon("tiles", None).for_local_view(),
    TabMenuRow::new("view.place", "design_view.place", "").with_icon("tiles", None).needing_others().for_local_view(),
    MOVE_TO_WINDOW_MENU,
    TabMenuRow::new(VIEW_CLOSE, "Close view", "Ctrl+W").with_icon("close", None).separated(),
    TabMenuRow::new(VIEW_CLOSE_OTHERS, "Close other views", "").needing_others().for_local_view(),
    TabMenuRow::new(VIEW_CLOSE_RIGHT, "Close views to the right", "").needing_others().for_local_view(),
    TabMenuRow::new(DESIGN_CLOSE, "Close design", "Ctrl+Shift+W").with_icon("close", None).separated(),
];

pub(crate) fn window_menu_entries(app: &SolveApp) -> Vec<(String, String)> {
    crate::view_windows::window_choices(app).iter().map(|choice| (format!("{WINDOW_PREFIX}{}", choice.id), choice.label.clone())).collect()
}

pub(crate) fn window_menu_rows<'a>(app: &SolveApp, entries: &'a [(String, String)]) -> Vec<TabMenuRow<'a>> {
    let current = app.view_windows.current_window();
    let mut new = MOVE_TO_WINDOW_MENU.children[0];
    if !crate::view_windows::can_create(app) {
        new.disabled_tip = Some(app.tr("multi_window.too_many"));
    }
    [new, TabMenuRow::heading(app.tr("multi_window.open_windows")).separated()]
        .into_iter()
        .chain(entries.iter().map(|(id, label)| TabMenuRow { checked: window_menu_target(id) == Some(current), ..TabMenuRow::new(id, label, "") }))
        .collect()
}

pub(crate) fn window_menu_target(id: &str) -> Option<u32> {
    id.strip_prefix(WINDOW_PREFIX)?.parse().ok()
}

pub(crate) fn view_menu_rows<'a>(app: &SolveApp, window_rows: &'a [TabMenuRow<'a>]) -> Vec<TabMenuRow<'a>> {
    let pop_out = app.view_windows.is_child();
    let can_move = crate::view_windows::supports_pop_out(app);
    VIEW_MENU
        .into_iter()
        .filter(|row| (row.id != VIEW_MOVE_TO_WINDOW || can_move) && (row.id != DESIGN_CLOSE || !pop_out))
        .map(|row| if row.id == VIEW_MOVE_TO_WINDOW { row.with_children(window_rows) } else { row })
        .collect()
}

/// The "+" menu: a header, one row per view kind with its icon in the kind's accent (`dark` picks
/// the accent for the menu's ground), then copy the shown view and bring back the last closed one.
pub fn plus_menu(dark: bool) -> Vec<TabMenuRow<'static>> {
    let kind = |id, label: &'static str, icon| TabMenuRow::new(id, label, "").with_icon(icon, Some(crate::workspace::view_icon_colour(label, dark)));
    vec![
        TabMenuRow::heading("Open as a view"),
        kind("plus.model", "Model", "model"),
        kind("plus.sketch", "Sketch", "sketch"),
        kind("plus.sheet", "Sheet", "sheet"),
        kind("plus.render", "Render", "render"),
        kind("plus.animation", "Animation", "animation"),
        TabMenuRow::new(PLUS_BESIDE, "Another angle of this view", "").with_icon("tiles", None).separated(),
        TabMenuRow::new(PLUS_DUPLICATE, "Duplicate this view", "").with_icon("duplicate", None),
        TabMenuRow::new(PLUS_REOPEN, "Reopen closed view", "Ctrl+Shift+T").with_icon("reopen", None),
    ]
}

/// The Layout button's menu: one row for each way to arrange the views in the window.
pub fn layout_menu() -> Vec<TabMenuRow<'static>> {
    vec![
        TabMenuRow::new("layout.single", "Single view", ""),
        TabMenuRow::new("layout.side_by_side", "Two side by side", ""),
        TabMenuRow::new("layout.stacked", "Two stacked", ""),
        TabMenuRow::new("layout.large_and_two", "One large and two small", ""),
        TabMenuRow::new("layout.grid", "Two by two", ""),
        TabMenuRow::new("fill.angle", "A new angle of this view", ""),
        TabMenuRow::new("fill.standard", "Standard angles: front, right, top", ""),
        TabMenuRow::new("fill.picker", "Empty cells that ask", ""),
    ]
}

/// The id of the shown design's view at place `index`.
fn view_id(app: &SolveApp, index: usize) -> Option<u64> {
    documents::strip_view_id(app, index)
}

/// Carry out one of the strip's view actions. False for an action that is not a view action.
pub fn handle(app: &mut SolveApp, _ctx: &egui::Context, action: TabAction) -> bool {
    match action {
        TabAction::SelectView(i) => {
            if let Some(id) = view_id(app, i) {
                documents::view_activate(app, id);
            }
        }
        TabAction::ReturnView(i) => {
            if let Some(id) = view_id(app, i) {
                let window = app.view_windows.current_window();
                move_to_window(app, id, serde_json::json!(window));
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
        TabAction::ReorderView { from, to } => {
            let local_index = |i| {
                let id = view_id(app, i)?;
                app.docs.views.views().iter().position(|view| view.id == id)
            };
            if let Some((from, to)) = local_index(from).zip(local_index(to)) {
                documents::view_move(app, from, to);
            }
        }
        TabAction::ViewMenu { view, id } => return view_menu(app, view, &id),
        TabAction::PlusMenu { id } | TabAction::LayoutMenu { id } => return plus_row(app, &id),
        TabAction::SwitcherMenu { id } => return switcher_menu(app, &id),
        TabAction::TearOff(i) => {
            if !app.file_picker_busy() {
                documents::switch(app, i);
                if app.docs.active == i {
                    open_new_window(app, None);
                }
            }
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
        VIEW_BESIDE => {
            let beside = (view != app.docs.views.active().id).then_some(view);
            return documents::view_beside(app, beside, solidcraft_views::Edge::Right).is_some();
        }
        "view.place" => {
            app.docs.view_placement = Some((app.docs.active_tab(), view));
        }
        VIEW_NEW_WINDOW => move_to_window(app, view, serde_json::json!("new")),
        id if let Some(window) = window_menu_target(id) => move_to_window(app, view, serde_json::json!(window)),
        VIEW_CLOSE => {
            if !crate::view_windows::defer_close_view(app, view) {
                documents::view_close(app, view);
            }
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
        // A Model view is another angle on the design, so "+ Model" always opens a new one; the
        // Render workspace is one place, so it is reused.
        let reuse = kind == ViewKind::Render && app.preferences.design_reuse_views;
        documents::view_open(app, kind, None, reuse);
        return true;
    }
    if let Some(fill) = documents::LayoutFill::ALL.into_iter().find(|fill| fill.key() == id) {
        app.docs.layout_fill = fill;
        return true;
    }
    if let Some(preset) = id.strip_prefix(LAYOUT_PREFIX).and_then(solidcraft_views::Preset::from_key) {
        documents::view_layout(app, preset);
        return true;
    }
    match id {
        PLUS_BESIDE => {
            documents::view_beside(app, None, solidcraft_views::Edge::Right);
        }
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
        VIEW_NEW_WINDOW => {
            let view = app.docs.views.active().id;
            move_to_window(app, view, serde_json::json!("new"));
        }
        id if let Some(window) = window_menu_target(id) => {
            move_to_window(app, app.docs.views.active().id, serde_json::json!(window));
        }
        SWITCHER_AS_TABS => app.preferences.design_views_as_tabs = true,
        _ => return false,
    }
    true
}

pub(crate) fn move_to_window(app: &mut SolveApp, view: u64, window: serde_json::Value) {
    if let Err(reason) = app.run("tab.view.move_to_window", serde_json::json!({"view": view, "window": window})) {
        show_move_error(app, reason);
    }
}

pub(crate) fn show_move_error(app: &mut SolveApp, reason: String) {
    let message = match reason.as_str() {
        "last_view_of_main_window" => app.tr("multi_window.last_view").to_owned(),
        "too_many_windows" => app.tr("multi_window.too_many").to_owned(),
        "windows_unavailable" => app.tr("multi_window.unavailable").to_owned(),
        _ => reason,
    };
    app.set_status(message, true);
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
    fn tearing_off_a_tab_stops_when_a_file_picker_refuses_the_switch() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        documents::open_tab(&mut h.app, solidcraft_engine::Session::default()).unwrap();
        documents::switch(&mut h.app, 0);
        h.app.services.pick_open = Some(Box::new(|_| Box::pin(std::future::pending())));
        h.app.pick_open(|_, _| {});
        assert!(h.app.file_picker_busy());
        h.app.status = None;
        assert!(handle(&mut h.app, &h.ctx, TabAction::TearOff(1)));
        assert_eq!(h.app.docs.active, 0);
        assert!(h.app.status.is_none(), "the refused switch does not try to open the active design");
    }

    #[test]
    fn menu_ids_are_unique_and_plus_rows_name_every_kind() {
        let plus = plus_menu(false);
        let layouts = layout_menu();
        let ids: Vec<&str> = VIEW_MENU.iter().chain(plus.iter()).chain(layouts.iter()).map(|r| r.id).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len());
        for kind in ViewKind::ALL {
            assert!(plus.iter().any(|r| r.id.strip_prefix("plus.") == Some(kind.key()) && r.icon == kind.key() && r.accent.is_some()));
        }
        assert!(plus[0].heading, "the menu opens with a header");
        assert!(plus.iter().filter(|r| r.heading).count() == 1, "and has no other");
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
