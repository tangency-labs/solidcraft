//! Application bar, the workspace toolbar (tabs and panels) and keyboard shortcuts.

use crate::ui_kit::BackingTooltipExt as _;
use crate::ui_kit::{self, Container, Edge, Surface};
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;

use crate::ribbon::{self, BUTTON_MIN_WIDTH, ButtonSize, GROUP_HEIGHT, ICON_GROUP_HEIGHT};
use crate::{SolveApp, app_bar, icons};
use solidcraft_theme::Tokens;

pub use crate::workspace::TABS;

/// Accent for a tab or a panel with a more specific semantic role.
fn panel_accent(t: &Tokens, tab: &str, panel: &str) -> Color32 {
    if panel == "CONSTRAIN" {
        return t.glyph_text;
    }
    crate::workspace::tab_accent(t, tab).color
}

/// Panels shown on each tab (in order). Commands come from the registry by (tab, panel).
pub fn panels(tab: &str) -> Vec<&'static str> {
    crate::workspace::panels(tab)
}

pub use crate::ribbon::TAB_ROW_HEIGHT;
pub const RIBBON_HEIGHT: f32 = TAB_ROW_HEIGHT + GROUP_HEIGHT + 1.0;
pub(crate) const WORKSPACE_WIDTH: f32 = 88.0;

/// The same height drives shell reservations and ribbon painting in either layout.
pub(crate) fn ribbon_height(labels: bool) -> f32 {
    if labels { RIBBON_HEIGHT } else { TAB_ROW_HEIGHT + ICON_GROUP_HEIGHT + 1.0 }
}

/// The commands a panel's drop-down (or its » submenu) lists: its own, then any button moved
/// into it from another panel.
fn menu_commands(
    cmds: &[&'static solidcraft_engine::CommandSpec],
    shown: &[&'static solidcraft_engine::CommandSpec],
) -> Vec<&'static solidcraft_engine::CommandSpec> {
    let mut out = cmds.to_vec();
    for c in shown {
        if !out.iter().any(|o| o.id == c.id) {
            out.push(*c);
        }
    }
    out
}

/// The command whose tool or dialog is open right now, shown pressed in the toolbar.
fn running_command(app: &SolveApp) -> Option<String> {
    match (&app.tool, &app.dialog) {
        (Some(tool), _) => Some(tool.cmd.to_string()),
        // Create Sketch never lands in last_command (it is not repeatable).
        (None, Some(d)) if matches!(d.kind, crate::dialogs::Kind::Sketch) => Some("sketch.create".to_string()),
        (None, Some(_)) => app.last_command.as_ref().map(|(id, _)| id.clone()),
        _ => None,
    }
}

/// Whether the toolbar shows command `id` pressed: its tool or dialog is open, or it turned on
/// the view analysis on now (one at a time, `Session::active_analysis`), its tool running or not.
pub(crate) fn shows_pressed(app: &SolveApp, id: &str) -> bool {
    running_command(app).as_deref() == Some(id) || app.session.active_analysis() == Some(id)
}

/// [`shows_pressed`] for a ribbon button: a command with a persistent effect is pressed only while
/// its tool runs, since `persistent_ribbon_pressed` alone decides how its effect is shown.
fn ribbon_shows_pressed(app: &SolveApp, id: &str) -> bool {
    if persistent_available(app, id) { running_command(app).as_deref() == Some(id) } else { shows_pressed(app, id) }
}

/// The height of an application-bar pill ([`offer_pill`]), a little under the bar's 26 px buttons.
pub(crate) const PILL_HEIGHT: f32 = 22.0;

/// An application-bar pill for something on offer (Update) at the right of `span` (the bar's free
/// space): accent-filled, with `icon` and the first of `labels` that fits (the icon alone when
/// none does or `labeled` is false). Every label but the last also leaves `keep` points of the
/// span free (for the document tabs). None when even the icon doesn't fit: it never covers the
/// bar's other buttons. `handle` names it for scenarios.
pub(crate) fn offer_pill(
    ui: &mut egui::Ui,
    span: Rect,
    labeled: bool,
    labels: &[&str],
    keep: f32,
    icon: &str,
    handle: &str,
) -> Option<(Rect, egui::Response)> {
    let t = Tokens::get();
    let font = FontId::proportional(t.font_label);
    let (icon_x, text_x, right_pad) = (16.0, 29.0, 11.0);
    let last = labels.len().saturating_sub(1);
    let label = labels.iter().enumerate().filter(|_| labeled).find_map(|(i, text)| {
        let galley = ui.fonts_mut(|f| f.layout_no_wrap(text.to_string(), font.clone(), t.app_bar_text));
        let width = text_x + galley.size().x + right_pad;
        let room = if i < last { span.width() - keep } else { span.width() };
        (width <= room).then_some((*text, width))
    });
    let width = label.map_or(PILL_HEIGHT + 4.0, |(_, w)| w);
    if width > span.width() {
        return None;
    }
    let rect = Rect::from_min_size(pos2(span.right() - width, span.center().y - PILL_HEIGHT / 2.0), vec2(width, PILL_HEIGHT));
    let response = ui.interact(rect, ui.scope_id().with(("sc_offer_pill", handle)), Sense::click());
    let name = label.map(|(text, _)| text).or(labels.first().copied()).unwrap_or_default().to_string();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), &name));
    crate::scenario::publish_target(handle, rect);
    let hot = response.hovered() || response.has_focus();
    let fill = if hot { t.app_bar_hover_button } else { t.accent };
    let radius = PILL_HEIGHT / 2.0 * f32::from(t.radius_xl) / 10.0;
    ui.painter().rect_filled(rect, radius, fill);
    if response.has_focus() {
        ui.painter().rect_stroke(rect.expand(2.0), Tokens::outset_radius(radius, 2.0), Stroke::new(1.0, t.app_bar_text), egui::StrokeKind::Outside);
    }
    let glyph_x = if label.is_some() { rect.left() + icon_x } else { rect.center().x };
    let glyph = Rect::from_center_size(pos2(glyph_x, rect.center().y), vec2(16.0, 16.0));
    icons::paint_on(ui.painter(), glyph, icon, Some(true), 1.0);
    if let Some((text, _)) = label {
        ui.painter().text(pos2(rect.left() + text_x, rect.center().y), Align2::LEFT_CENTER, text, font, t.app_bar_text);
    }
    Some((rect, response))
}

/// The application bar's height: the window's title bar on desktop builds.
pub use crate::app_bar::HEIGHT as APP_BAR_HEIGHT;

/// Where the application bar puts command search, Report a bug and the version: chosen by the
/// app that hosts the UI ([`SolveApp::header`]) before the first frame. The default is the
/// desktop layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeaderLayout {
    pub search: SearchPlace,
    pub report: ReportButton,
    pub version: VersionPlace,
}

impl HeaderLayout {
    /// Search in the ribbon's tab row, a labelled Report a bug, the version in the bar.
    pub const DESKTOP: HeaderLayout = HeaderLayout { search: SearchPlace::Ribbon, report: ReportButton::Labelled, version: VersionPlace::Header };
    /// Search in the bar beside Settings, Report a bug as an icon, the version in the Help menu.
    pub const COMPACT: HeaderLayout = HeaderLayout { search: SearchPlace::Header, report: ReportButton::Icon, version: VersionPlace::HelpMenu };
}

/// Where the closed command search field sits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchPlace {
    /// At the right end of the ribbon's tab row.
    #[default]
    Ribbon,
    /// In the application bar, immediately left of Settings; just its magnifier when the bar is
    /// short of room. The ribbon keeps it where the bar has no Settings button of its own (a
    /// system File menu) or is not shown (an embedded view).
    Header,
}

/// How the application bar shows Report a bug.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReportButton {
    /// A button with its label when there is room for it (the icon alone otherwise).
    #[default]
    Labelled,
    /// Always the icon alone, drawn like Settings and Help; its name stays the tooltip and the
    /// accessible label.
    Icon,
}

/// Where the version is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VersionPlace {
    /// At the application bar's right end, linking to the release notes.
    #[default]
    Header,
    /// As a row of the Help menu, opening the release notes.
    HelpMenu,
}

/// The closed search field goes in the application bar rather than the ribbon.
pub(crate) fn search_in_header(app: &SolveApp) -> bool {
    app.header.search == SearchPlace::Header && !app.embedded && !app.embed_editor
}

/// Where SolidCraft's application bar publishes its parts (scripts and scenarios use these names;
/// `appbar:folder` and `tab:home` are kept for scripts written against the former folder button
/// and Home tab).
const APP_BAR_HANDLES: app_bar::Handles<'static> = app_bar::Handles {
    home: &["appbar:home", "titlebar:home", "tab:home"],
    home_rect: &["titlebar:home", "appbar:home"],
    file: &["appbar:file", "appbar:folder"],
    file_rect: &["appbar:file"],
    plugins: "appbar:plugins",
    button: &["appbar:", "titlebar:"],
    separator: "titlebar:separator:",
    search: "titlebar:search",
    report: "titlebar:report",
    settings: "titlebar:preferences",
    version: "titlebar:version",
};

pub fn app_bar(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let app_bar = Container::new(ui.ctx(), Surface::ApplicationBar).edge("sc_appbar", Edge::Top);
    let frame = Container::new(ui.ctx(), Surface::ApplicationBar).edge_frame(t.app_bar);
    Container::new(ui.ctx(), Surface::ApplicationBar).show_edge(ui, app_bar.exact_size(APP_BAR_HEIGHT).show_separator_line(false), frame, |ui| {
        let r = ui.max_rect();
        // The bar is the window's title bar: its empty space moves the window, the traffic lights
        // or the caption buttons take its ends.
        let chrome = crate::window_chrome::TitleBar { traffic_lights: app.integrated_titlebar, caption_buttons: app.custom_titlebar };
        let action = chrome.show(ui, r, |ui, content| {
            // While a file loads the design waits: no menus or commands from the bar.
            let idle = !app.file_load_holds();
            // Save, Undo and Redo use the same actions as the File menu and shortcuts.
            let shown = design_shown(app) && idle;
            let tip = |id: &str, key: &str| {
                let label = app.tr(id);
                if shown { format!("{label} ({key})") } else { solidcraft_ui_text::format("ui.needs_design", &[("command", label)]) }
            };
            let (save_tip, undo_tip, redo_tip) = (tip("ui.save", "Ctrl+S"), tip("ui.undo", "Ctrl+Z"), tip("ui.redo", "Ctrl+Y"));
            let prefs_tip = if cfg!(target_os = "macos") {
                format!("{} (Cmd+,)", app.tr("ui.preferences"))
            } else {
                format!("{} (Ctrl+,)", app.tr("ui.preferences"))
            };
            let version = format!("v{}", crate::update::current_version());
            let url = crate::update::release_notes_url(crate::update::current_version(), crate::update::release_build());
            let file_open = ui.ctx().data(|d| d.get_temp::<bool>(egui::Id::unique("sc_file_menu")).unwrap_or(false));
            let quick = app.toolbar_custom.quick_access.iter().filter(|id| solidcraft_engine::find_command(id).is_some()).count();
            let action = |id, icon, tip, enabled, wide_only| app_bar::BarButton { id, icon, tip, enabled, narrow: true, wide_only };
            let palette_shortcut = crate::keymap::display_for(app, "ui.palette");
            // Narrow windows retain Save; Undo and Redo remain available from their shortcuts and menus.
            let bar = app_bar::AppBar {
                mark: &|painter, at| solidcraft_look::glyphs::brand_mark(painter, at),
                name: "SolidCraft",
                home: app_bar::BarButton { id: "home", icon: "home", tip: app.tr("ui.start_page"), enabled: idle, narrow: false, wide_only: false },
                file: app.tr("ui.file"),
                file_open,
                // The web build runs no plugins: no Plugins menu there. Its accessible name says
                // "menu", so it is not taken for Preferences' Plugins page.
                plugins: (!cfg!(target_arch = "wasm32")).then(|| (app.tr("ui.plugins"), app.plugin_menu.open, app.tr("ui.plugin_menu_name"))),
                actions: vec![
                    action("save", "save", &save_tip, shown, false),
                    action("undo", "undo", &undo_tip, shown, true),
                    action("redo", "redo", &redo_tip, shown, true),
                ],
                quick,
                help: app_bar::BarButton { id: "help", icon: "help", tip: app.tr("ui.help"), enabled: true, narrow: false, wide_only: false },
                settings: app_bar::BarButton { id: "settings", icon: "settings", tip: &prefs_tip, enabled: true, narrow: false, wide_only: false },
                report: app_bar::Report {
                    label: solidcraft_ui_text::tr("ui.report_a_bug"),
                    tip: solidcraft_ui_text::tr_label("Report a bug — review a screenshot and open a GitHub draft"),
                    icon_only: app.header.report == ReportButton::Icon,
                },
                search: search_in_header(app) && design_shown(app),
                search_shortcut: palette_shortcut.as_deref(),
                assistant: crate::agent_chat::app_bar_button_shown(app),
                version: (app.header.version == VersionPlace::Header).then_some(app_bar::Version { text: &version, url: &url }),
                handles: APP_BAR_HANDLES,
            };
            let out = app_bar::show(ui, &bar, r, content);
            app.plugin_menu.in_file_menu = out.plugins_in_file;
            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("sc_file_menu_x"), out.file.left()));
            for action in &out.actions {
                match action {
                    app_bar::BarAction::Home => app.home.show(app.session.revision),
                    app_bar::BarAction::Plugins { rect } => {
                        app.ui.palette_open = false;
                        crate::plugin_menu::toggle(app, &ui.ctx().clone(), *rect);
                    }
                    app_bar::BarAction::File { .. } => {
                        app.ui.palette_open = false;
                        app.plugin_menu.open = false;
                        if app.plugin_menu.in_file_menu {
                            crate::plugin_menu::refresh(app);
                        }
                        let ctx = ui.ctx().clone();
                        let open = ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("sc_file_menu")).unwrap_or(false));
                        let pass = ctx.cumulative_pass_nr();
                        // The File button toggles the menu. Remember the pass it opened on so the same
                        // click isn't also treated as a click outside the menu (which would close it at once).
                        ctx.data_mut(|d| {
                            d.insert_temp(egui::Id::unique("sc_file_menu"), !open);
                            d.insert_temp(egui::Id::unique("sc_file_menu_opened"), pass);
                        });
                    }
                    app_bar::BarAction::Button { id } => match id.as_str() {
                        "save" => save(app),
                        "undo" => FileAction::Undo.run(app),
                        "redo" => FileAction::Redo.run(app),
                        "settings" => crate::prefs::open_at(app, 0),
                        _ => {}
                    },
                    app_bar::BarAction::Help { x } => {
                        app.help.menu = !app.help.menu;
                        ui.ctx().data_mut(|d| {
                            d.insert_temp(egui::Id::unique("sc_help_opened"), true);
                            d.insert_temp(egui::Id::unique("sc_help_anchor"), *x);
                        });
                    }
                    app_bar::BarAction::Report => crate::help::run(app, &ui.ctx().clone(), "report"),
                    app_bar::BarAction::Version => ui.ctx().open_url(egui::OpenUrl::new_tab(&url)),
                }
            }
            if let Some((first, step)) = out.quick {
                quick_access_bar(app, ui, r, first, step, shown);
            }
            if let Some(search) = out.search {
                // shortcut_box::show draws the one search editor wherever this rectangle is.
                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("sc_ribbon_search"), search));
            }
            // A newer release: its Update button stays before the right cluster.
            let update = crate::update::button(app, ui, out.offer, out.offer_labeled);
            let tabs_end = update.map_or(out.controls_left, |u| u.left()) - 8.0;
            // Narrow title bars can fit only part of the tab strip. Its hit regions must not
            // extend into the persistent report and window controls.
            let tabs_rect = Rect::from_min_max(pos2(out.tabs_start.min(tabs_end), r.top()), pos2(tabs_end, r.bottom()));
            let mut tabs_ui = ui.new_child(egui::UiBuilder::new().max_rect(tabs_rect));
            tabs_ui.set_clip_rect(tabs_rect.intersect(ui.clip_rect()));
            crate::documents::tabs(app, &mut tabs_ui, r, out.tabs_start, tabs_end);
            if let Some(at) = out.assistant {
                assistant_button(app, ui, at);
            }
        });
        if let Some(action) = action {
            crate::titlebar::run(app, ui.ctx(), action);
        }
    });
    file_menu(app, ui.ctx());
    crate::plugin_menu::menu(app, &ui.ctx().clone());
    let help_x = ui.ctx().data(|d| d.get_temp::<f32>(egui::Id::unique("sc_help_anchor")).unwrap_or(300.0));
    crate::help::show(app, &ui.ctx().clone(), pos2(help_x - 14.0, APP_BAR_HEIGHT));
}

/// File, edit and application actions shared by the in-window and native menus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileAction {
    /// Create a design using the document lifecycle.
    New,
    /// Pick a design or an exchange file to open.
    Open,
    /// Save the active design, asking for a path when needed.
    Save,
    /// Save the active design under another name.
    SaveAs,
    /// Undo the active design's last command.
    Undo,
    /// Redo the active design's last undone command.
    Redo,
    /// Save a JSON design using the existing save-as flow.
    SaveJson,
    /// Export through the format-specific export command.
    Export(&'static str),
    /// Open the mesh export dialog with the selected format.
    ExportMesh(&'static str),
    /// Start an existing interactive command.
    Command(&'static str),
    /// Open an entry from the recent-design list.
    OpenRecent(String),
    /// Clear recent designs and their last-opened dates.
    ClearRecent,
    /// Show the application's preferences.
    Settings,
    /// Show the keyboard shortcut editor.
    Shortcuts,
    /// Toggle the agent chat panel.
    AgentChat,
    /// Request application exit, checking for unsaved documents.
    Quit,
}

impl FileAction {
    /// Whether this action is available in the current document, picker and report state.
    pub fn enabled(&self, app: &SolveApp) -> bool {
        let needs_design = matches!(self, Self::Save | Self::SaveAs | Self::Undo | Self::Redo | Self::ExportMesh(_) | Self::Command(_))
            || matches!(self, Self::Export(extension) if *extension != "json");
        // An open bug report describes the designs as they are, and a window opened behind it
        // covers it (#7188). Quit too, as behind any modal window: it would switch to a design
        // with unsaved changes under the report, and ask about it where it can't be answered.
        let behind_report = app.issue_report.draft.is_some();
        let available = match self {
            Self::Undo => !app.session.undo.is_empty(),
            Self::Redo => !app.session.redo.is_empty(),
            Self::ClearRecent => !app.home.recent.is_empty(),
            Self::AgentChat => crate::agent_chat::app_bar_button_shown(app),
            _ => true,
        };
        !app.file_picker_busy() && !app.file_load_holds() && !behind_report && (!needs_design || design_shown(app)) && available
    }

    /// Whether the native menu offers this action: while a widget has keyboard focus (a report's
    /// text field too), its Undo and Redo go to that widget instead of the design.
    pub fn menu_enabled(&self, app: &SolveApp, keyboard_focus: bool) -> bool {
        self.enabled(app) || (matches!(self, Self::Undo | Self::Redo) && keyboard_focus && !app.file_picker_busy() && !app.file_load_holds())
    }

    /// Execute through the same document, save-job and preferences paths as the toolbar.
    pub fn run(&self, app: &mut SolveApp) {
        if !self.enabled(app) {
            return;
        }
        match self {
            Self::New => crate::documents::new_design(app),
            Self::Open => app.pick_open(|app, path| {
                if let Some(path) = path {
                    app.open_path_async(&path);
                }
            }),
            Self::Save => save(app),
            Self::SaveAs => save_as(app),
            Self::Undo => drop(app.run("edit.undo", json!({}))),
            Self::Redo => drop(app.run("edit.redo", json!({}))),
            Self::SaveJson => save_as_with_extension(app, "json"),
            Self::Export(extension) => export(app, extension),
            Self::ExportMesh(format) => crate::dialogs_save_mesh::start_with_format(app, format),
            Self::Command(command) => app.start(command),
            Self::OpenRecent(path) => crate::home::open_recent(app, path),
            Self::ClearRecent => {
                app.home.recent.clear();
                app.home.opened_at.clear();
            }
            Self::Settings => crate::prefs::open_at(app, 0),
            Self::Shortcuts => {
                app.keymap.open = true;
                app.keymap.read_only = false;
            }
            Self::AgentChat => crate::agent_chat::toggle(app),
            Self::Quit => crate::documents::request_quit(app),
        }
    }
}

fn file_menu(app: &mut SolveApp, ctx: &egui::Context) {
    let id = egui::Id::unique("sc_file_menu");
    if !ctx.data(|d| d.get_temp::<bool>(id).unwrap_or(false)) {
        return;
    }
    let rows = file_rows(app);
    let at = pos2(ctx.data(|d| d.get_temp::<f32>(egui::Id::unique("sc_file_menu_x"))).unwrap_or(100.0), APP_BAR_HEIGHT);
    let just_opened = ctx.data(|d| d.get_temp::<u64>(egui::Id::unique("sc_file_menu_opened"))) == Some(ctx.cumulative_pass_nr());
    let out = app_bar::file_menu(ctx, &rows, at, egui::Id::unique("sc_file_area"), just_opened);
    if let Some(chosen) = out.chosen {
        run_file_row(app, &chosen);
    }
    if out.close {
        ctx.data_mut(|d| d.insert_temp(id, false));
    }
}

/// The File menu's rows, each named by what it runs ([`run_file_row`]).
fn file_rows(app: &SolveApp) -> Vec<app_bar::FileRow> {
    use app_bar::{FileItem, FileRow};
    let row = |id: &str, label: &str, key: &str, icon: &str, enabled: bool| {
        FileRow::Item(FileItem { id: id.to_string(), label: label.to_string(), key: key.to_string(), icon: icon.to_string(), enabled })
    };
    let button = |id: &str, label: &str, icon: &str, tip: Option<&str>| FileRow::Button {
        item: FileItem { id: id.to_string(), label: label.to_string(), key: String::new(), icon: icon.to_string(), enabled: true },
        tip: tip.map(str::to_string),
    };
    // Insert, Save and Export act on the design, which Home hides.
    let shown = design_shown(app);
    let mut rows =
        vec![row("new", app.tr("ui.new_design"), "Ctrl+N", "new", true), row("open", &format!("{}…", app.tr("ui.open")), "Ctrl+O", "open", true)];
    let mut recent: Vec<FileRow> = app
        .home
        .recent
        .iter()
        .enumerate()
        .map(|(i, path)| {
            let missing = !crate::home::available(app, path);
            button(&format!("recent:{i}"), &format!("{path}{}", if missing { " (missing)" } else { "" }), "open", None)
        })
        .collect();
    recent.push(FileRow::Separator);
    recent.push(button("clear_recent", app.tr("ui.clear_recent"), "delete", None));
    rows.push(FileRow::Sub { label: app.tr("ui.open_recent").to_string(), rows: recent });
    rows.push(row("file.insert_step", &format!("{}…", app.tr_label("Insert STEP")), "", "import", shown));
    rows.push(row("file.insert_mesh", &format!("{}…", app.tr_label("Insert Mesh")), "", "import", shown));
    rows.push(row("pointcloud.insert", &format!("{}…", app.tr_label("Insert Point Cloud")), "", "import", shown));
    rows.push(row("component.insert_design", &format!("{}…", app.tr_label("Insert Design")), "", "component", shown));
    rows.push(row("save", app.tr("ui.save"), "Ctrl+S", "save", shown));
    // Save As shows the shortcut of the command it runs, from the app's own registry.
    let save_as_key = crate::keymap::effective(app, "file.save_as").unwrap_or_default();
    rows.push(row("save_as", &format!("{}…", app.tr("ui.save_as")), &save_as_key, "save", shown));
    rows.push(row("save_json", app.tr_label("Save As JSON…"), "", "save", true));
    // The browser build: a copy as a downloaded file, whatever Save does.
    if app.services.download_design.is_some() {
        rows.push(row("download", app.tr_label("Download design"), "", "export", shown));
    }
    if app.services.browser_files.is_some() {
        rows.push(row("browser_files", "Files in this browser…", "", "open", true));
    }
    rows.push(FileRow::Separator);
    // Each format's item runs its own command, so it shows that command's key.
    let key = |id: &str| crate::keymap::effective(app, id).unwrap_or_default();
    rows.push(row("file.export_stl", app.tr_label("Export STL…"), &key("file.export_stl"), "mesh_export", shown));
    rows.push(row("file.export_3mf", app.tr_label("Export 3MF…"), &key("file.export_3mf"), "mesh_export", shown));
    rows.push(row("file.send_to_slicer", app.tr("cmd.file_send_to_slicer"), "", "mesh_export", shown && crate::dialogs_save_mesh::available(app)));
    rows.push(row("file.export_step", app.tr_label("Export STEP…"), &key("file.export_step"), "export", shown));
    rows.push(row("file.export_view_dxf", app.tr_label("Export Current View as DXF…"), "", "export", shown));
    rows.push(row("file.export_obj", app.tr_label("Export OBJ…"), &key("file.export_obj"), "export", shown));
    rows.push(row("export_glb", &format!("{}…", app.tr("ui.export_glb")), "", "export", shown));
    rows.push(row("file.export_urdf", app.tr_label("Export URDF…"), &key("file.export_urdf"), "export", shown));
    rows.push(row("export_json", app.tr_label("Export Design as JSON…"), "", "export", true));
    rows.push(FileRow::Separator);
    rows.push(row("document.metadata", &format!("{}…", app.tr_label("Design Properties")), "", "settings", shown));
    rows.push(FileRow::Separator);
    // A narrow app bar has no room for the Plugins button: its menu is here instead.
    if app.plugin_menu.in_file_menu {
        rows.push(crate::plugin_menu::file_submenu(app));
        rows.push(FileRow::Separator);
    }
    // The browser can install SolidCraft as an app (`install`): only while it offers to.
    if crate::install::offered(app).is_some() {
        rows.push(button("install", solidcraft_ui_text::tr("install.menu"), "new", Some(solidcraft_ui_text::tr("install.button_tip"))));
    }
    rows.push(row("settings", &format!("{}…", app.tr("ui.preferences")), "", "settings", true));
    rows.push(row("shortcuts", &format!("{}…", app.tr("ui.keyboard_shortcuts")), "", "list", true));
    rows.push(row("quit", app.tr("ui.quit"), "", "close", true));
    rows
}

/// Runs the File menu's row `id` ([`file_rows`]).
fn run_file_row(app: &mut SolveApp, id: &str) {
    if let Some(id) = id.strip_prefix("plugins:") {
        crate::plugin_menu::choose(app, id);
        return;
    }
    if let Some(i) = id.strip_prefix("recent:").and_then(|i| i.parse::<usize>().ok()) {
        if let Some(path) = app.home.recent.get(i).cloned() {
            FileAction::OpenRecent(path).run(app);
        }
        return;
    }
    match id {
        "new" => FileAction::New.run(app),
        "open" => FileAction::Open.run(app),
        "clear_recent" => FileAction::ClearRecent.run(app),
        "file.insert_step" => FileAction::Command("file.insert_step").run(app),
        "file.insert_mesh" => FileAction::Command("file.insert_mesh").run(app),
        "component.insert_design" => FileAction::Command("component.insert_design").run(app),
        "file.export_stl" => FileAction::Command("file.export_stl").run(app),
        "file.export_3mf" => FileAction::Command("file.export_3mf").run(app),
        "file.export_step" => FileAction::Command("file.export_step").run(app),
        "file.export_view_dxf" => FileAction::Command("file.export_view_dxf").run(app),
        "file.export_obj" => FileAction::Command("file.export_obj").run(app),
        "file.export_urdf" => FileAction::Command("file.export_urdf").run(app),
        "document.metadata" => FileAction::Command("document.metadata").run(app),
        "pointcloud.insert" | "file.send_to_slicer" => app.start(id),
        "save" => FileAction::Save.run(app),
        "save_as" => FileAction::SaveAs.run(app),
        "save_json" => FileAction::SaveJson.run(app),
        "download" => download_design(app),
        "browser_files" => {
            if let Some(show) = &app.services.browser_files {
                show();
            }
        }
        "export_glb" => FileAction::Export("glb").run(app),
        "export_json" => FileAction::Export("json").run(app),
        "install" => {
            if let Some(offer) = crate::install::offered(app) {
                crate::install::start(app, offer);
            }
        }
        "settings" => FileAction::Settings.run(app),
        "shortcuts" => FileAction::Shortcuts.run(app),
        "quit" => FileAction::Quit.run(app),
        _ => {}
    }
}

/// Whether the design is on screen: viewport/menu editing stays disabled behind File.
/// File owns its explicit Save/Save As/export actions and returns to the design before dispatch.
pub fn design_shown(app: &SolveApp) -> bool {
    !app.home.open
}

pub fn save(app: &mut SolveApp) {
    if crate::browser::needs_capture(app, crate::browser::SAVE) {
        app.tree.capture_prompt = Some(crate::browser::SAVE.into());
        return;
    }
    if app.session.path.is_some() {
        // Read-only, or changed on disk since it was opened: Save As, or a question, instead.
        if crate::open_elsewhere::check_save(app) {
            crate::save_job::start(app, "file.save", json!({}));
        }
    } else {
        save_as(app);
    }
}

pub(crate) fn save_as(app: &mut SolveApp) {
    save_as_with_extension(app, solidcraft_engine::io::DESIGN_EXT);
}

/// Save As offering a copy's name ("Bracket copy"), so the file it is a copy of is kept.
pub(crate) fn save_copy(app: &mut SolveApp) {
    let name = solidcraft_ui_text::format("document.copy_name", &[("name", &app.session.save_as_name())]);
    save_as_named(app, &name, solidcraft_engine::io::DESIGN_EXT);
}

/// Save As offers the open file's own name, not the name stored in the design (#8329).
fn save_as_with_extension(app: &mut SolveApp, extension: &str) {
    let name = app.session.save_as_name();
    save_as_named(app, &name, extension);
}

pub(crate) fn save_as_named(app: &mut SolveApp, name: &str, extension: &str) {
    // The Capture Position question first, as Save asks it: the file can't hold a move not
    // captured yet (#9736).
    if crate::browser::needs_capture(app, crate::browser::SAVE) {
        app.tree.capture_prompt = Some(crate::browser::save_as_question(name, extension));
        return;
    }
    let name = format!("{name}.{extension}");
    app.pick_save(&name, &[solidcraft_engine::io::DESIGN_EXT, "json"], |app, path| {
        // A save under way answers a closing tab when it ends.
        if let Some(path) = path
            && crate::save_job::start(app, "file.save_as", json!({"path": path}))
        {
            return;
        }
        crate::documents::saved_for_close(app);
    });
}

/// File > Download design (browser build): the open design as a `.sc` file download.
pub(crate) fn download_design(app: &mut SolveApp) {
    if crate::browser::needs_capture(app, crate::browser::SAVE) {
        app.tree.capture_prompt = Some(crate::browser::DOWNLOAD.into());
        return;
    }
    let name = format!("{}.{}", app.session.doc.name, solidcraft_engine::io::DESIGN_EXT);
    let result = app
        .session
        .design_container(solidcraft_engine::io::DesignCompression::Fast, false, false)
        .map_err(|e| e.to_string())
        .and_then(|bytes| app.services.download_design.as_ref().map_or(Ok(()), |download| download(&name, &bytes)));
    match result {
        Ok(()) => app.set_status(format!("Downloaded {name}"), false),
        Err(e) => app.set_status(format!("Download design: {e}"), true),
    }
}

pub(crate) fn export(app: &mut SolveApp, ext: &str) {
    export_as(app, ext, None);
}

/// Each format's own export command, started from its key, command search or the File menu,
/// does what that File › Export item does: STEP, OBJ and URDF ask where to save, STL and 3MF
/// open the mesh dialog with the format chosen. `false`: `id` is no such command.
pub(crate) fn start_export(app: &mut SolveApp, id: &str) -> bool {
    let (extension, mesh) = match id {
        "file.export_step" => ("step", false),
        "file.export_obj" => ("obj", false),
        "file.export_urdf" => ("urdf", false),
        "file.export_stl" => ("stl", true),
        "file.export_3mf" => ("3mf", true),
        _ => return false,
    };
    // As the File menu's items: not while a file is being picked or loaded, or behind a bug report.
    if !FileAction::Export(extension).enabled(app) {
        return true;
    }
    if mesh {
        crate::dialogs_save_mesh::start_with_format(app, extension);
    } else {
        export(app, extension);
    }
    true
}

/// Export to a file with extension `ext`, in `format` when the extension alone doesn't name it.
fn export_as(app: &mut SolveApp, ext: &str, format: Option<&str>) {
    let body = crate::export_name::first_body(app);
    let name = format!("{}.{ext}", crate::export_name::offered(app, body.as_deref(), &app.session.doc.name));
    let format = format.map(str::to_string);
    app.pick_save(&name, &[ext], move |app, path| {
        let Some(path) = path else { return };
        let mut params = json!({"path": path});
        if let Some(format) = format {
            params["format"] = json!(format);
        }
        if app.run("file.export", params).is_ok() {
            app.set_status(format!("Exported {path}"), false);
        }
    });
}

/// Commands opt into persistent presentation; the engine status list remains the only truth.
const PERSISTENT_COMMANDS: &[&str] = &[
    "inspect.accessibility",
    "inspect.continuity",
    "inspect.curvature_map",
    "inspect.draft",
    "inspect.environment_map",
    "inspect.section",
    "inspect.zebra",
];

/// Component Colors is a plain on/off display setting: it shows pressed like the effects above,
/// but is no status item (nothing to clear with Esc), so it stays out of the list.
const COMPONENT_COLORS: &str = "inspect.colored_components";

fn persistent_command(id: &str) -> bool {
    PERSISTENT_COMMANDS.contains(&id) || id == COMPONENT_COLORS
}

fn persistent_available(app: &SolveApp, id: &str) -> bool {
    persistent_command(id) && (id != "inspect.section" || app.preferences.viewport_section_status)
}

pub(crate) fn persistent_pressed(app: &SolveApp, id: &str) -> bool {
    app.preferences.persistent_ribbon_pressed
        && persistent_available(app, id)
        && if id == COMPONENT_COLORS {
            app.session.colored_components.is_some()
        } else {
            app.session.status_items().iter().any(|item| item.command == id)
        }
}

pub(crate) fn persistent_button_name(app: &SolveApp, command: &solidcraft_engine::CommandSpec) -> String {
    let info = crate::render_workspace::interactive_command_info(app, command);
    let label = crate::workspace::ribbon_button_label(command);
    app.tr_label(if label == command.label { info.label } else { label }).to_string()
}

fn persistent_hint(app: &SolveApp, command: &solidcraft_engine::CommandSpec, enabled: bool) -> Option<String> {
    (enabled && command.id != COMPONENT_COLORS && persistent_pressed(app, command.id)).then(|| {
        let name = persistent_button_name(app, command);
        solidcraft_ui_text::Locale::for_language(&app.preferences.language).format("ui.viewport_status_clear_hint", &[("name", &name)])
    })
}

/// A ribbon button or grid icon whose command has a persistent effect, painted after every
/// command of the pass ran ([`ribbon::paint_deferred`]). These records never survive the frame.
struct PersistentRibbonPaint {
    command: &'static solidcraft_engine::CommandSpec,
    deferred: ribbon::Deferred,
    label: Option<String>,
    enabled: bool,
    disabled: Option<String>,
}

fn paint_persistent_ribbon(app: &SolveApp, ui: &egui::Ui, paints: Vec<PersistentRibbonPaint>, tab: &str, namespace: &str) {
    let t = Tokens::get();
    let pressed = app.preferences.persistent_ribbon_pressed;
    let mut tips = Vec::new();
    let items = paints
        .into_iter()
        .map(|paint| {
            let id = paint.command.id;
            let checkable = pressed && persistent_available(app, id);
            let name = if checkable {
                persistent_button_name(app, paint.command)
            } else {
                app.tr_label(crate::render_workspace::interactive_command_info(app, paint.command).label).to_string()
            };
            let look = ribbon::DeferredLook {
                id: id.to_string(),
                selected: persistent_pressed(app, id),
                // Resolve the genuine live tool after dispatch too: a click may have ended it.
                // The compatibility presentation keeps its original transient recipe.
                running: pressed && app.tool.as_ref().is_some_and(|tool| tool.cmd == id),
                focus_ring: pressed,
                label: paint.label,
                enabled: paint.enabled,
                name,
                checkable,
                accent: crate::workspace::tab_accent(&t, tab),
            };
            tips.push((paint.deferred.response.clone(), paint.command, paint.disabled));
            (paint.deferred, look)
        })
        .collect();
    ribbon::paint_deferred(ui, items, namespace);
    for (response, command, disabled) in tips {
        let _ = ribbon_tooltip(app, response, command, disabled.as_deref());
    }
}

/// Ribbon headings use the translated labels, with normal word casing instead of registry caps.
pub(crate) fn ribbon_label(app: &SolveApp, label: &str) -> String {
    let label = crate::workspace::word_cased(app.tr_label(crate::workspace::tab_label(label)));
    // Partial catalogs may already translate the word-cased name (for example "Model").
    app.tr_label(&label).to_string()
}

/// SKETCH while editing a sketch and SHEET METAL while a sheet body is active take the contextual
/// tab style; SHEET METAL is an ordinary tab otherwise. WELDMENTS selects itself on a frame member
/// but keeps the ordinary tab look, as MODEL has when selected.
pub(crate) fn contextual_tab(app: &SolveApp, tab: &str) -> bool {
    tab == "SKETCH" || (tab == "SHEET METAL" && crate::workspace::sheet_body(app).is_some())
}

/// The names a tab is also published under: the tabs it took over.
fn tab_aliases(tab: &str) -> &'static [&'static str] {
    match tab {
        "MODEL" => &["SOLID", "PLASTIC"],
        "SURFACE" => &["MESH"],
        "MANAGE" => &["UTILITIES"],
        _ => &[],
    }
}

/// Where SolidCraft's tab row publishes its parts (scripts and scenarios use these names).
const TAB_HANDLES: ribbon::TabHandles<'static> =
    ribbon::TabHandles { tab: "tab:", tab_rect: "toolbar_tab:", add: "ribbon:add_tab", action: "ribbon:" };

/// The Render workspace's job buttons after the search field.
const RENDER_JOBS: [&str; 2] = ["render.start", "render.cancel"];

/// Returns `Some(already shown)` when a tab was clicked.
fn ribbon_tabs(app: &mut SolveApp, ui: &mut egui::Ui, row: Rect) -> Option<bool> {
    let t = Tokens::get();
    let ids = crate::workspace::visible_tabs(app);
    let labels: Vec<String> = ids.iter().map(|tab| ribbon_label(app, tab)).collect();
    let render = crate::render_workspace::active(app);
    // The application bar may hold the search field instead ([`HeaderLayout`]): the tabs then
    // run to the row's end.
    let in_ribbon = !search_in_header(app);
    let job_labels = [app.tr_label("Render").to_string(), app.tr_label("Cancel").to_string()];
    let jobs: Vec<(&str, &str, Result<(), String>)> = if render {
        RENDER_JOBS.iter().zip(&job_labels).map(|(id, label)| (*id, label.as_str(), crate::render_workspace::job_ready(app, id))).collect()
    } else {
        Vec::new()
    };
    let add_tip = app.tr("prefs.toolbar_tabs").to_string();
    let out = {
        let row_data = ribbon::Tabs {
            tabs: ids
                .iter()
                .zip(&labels)
                .map(|(id, label)| ribbon::Tab {
                    id,
                    label,
                    accent: crate::workspace::tab_accent(&t, id),
                    contextual: contextual_tab(app, id),
                    aliases: tab_aliases(id),
                })
                .collect(),
            active: &app.ui.tab,
            // A plus after the last tab opens the toolbar's menu, so tabs can be shown or hidden
            // without knowing about the right-click; it stays when every tab is hidden. The
            // Render workspace's one tab can't be hidden.
            add: (!render).then_some(add_tip.as_str()),
            menu_open_from: &|rect| crate::context_menu::open_from(app, rect, &crate::context_menu::Target::Toolbar),
            search_width: if in_ribbon {
                crate::ui_kit::CommandSearch::closed_width(ui.ctx(), crate::keymap::display_for(app, "ui.palette").as_deref())
            } else {
                0.0
            },
            buttons: jobs
                .iter()
                .map(|(id, label, ready)| ribbon::TabButton { id, label, ready: ready.as_ref().map(|_| ()).map_err(String::as_str) })
                .collect(),
            floating: Container::for_layout(app.ui.layout, Surface::Panel).floating(),
            underline: matches!(app.ui.tab.as_str(), "SKETCH" | "SHEET METAL").then(|| panel_accent(&t, &app.ui.tab, "")),
            handles: TAB_HANDLES,
        };
        ribbon::tabs(ui, &row_data, row)
    };
    // The single editor is drawn on the foreground layer by shortcut_box::show, so it
    // can grow over the ribbon without being clipped to the tab row.
    if in_ribbon {
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("sc_ribbon_search"), out.search));
    }
    let mut clicked = None;
    for action in out.actions {
        match action {
            ribbon::TabAction::Select { index, already } => {
                if let Some(tab) = ids.get(index) {
                    clicked = Some(already);
                    app.ui.tab = tab.to_string();
                    // Floating ribbons draw after the viewport. An instant surface switch
                    // needs one more pass too; settled tabs never schedule their own frames.
                    ui.ctx().request_repaint();
                }
            }
            ribbon::TabAction::Menu { at } => crate::context_menu::open_for(app, at, crate::context_menu::Target::Toolbar),
            ribbon::TabAction::Add { rect } => {
                crate::context_menu::toggle_for(app, rect, rect.left_bottom(), crate::context_menu::Target::Toolbar);
            }
            ribbon::TabAction::Run { id } => app.start(&id),
        }
    }
    clicked
}

pub(crate) fn shell_layout(app: &SolveApp) -> crate::ui_kit::LayoutMode {
    if crate::render_workspace::active(app) { crate::ui_kit::LayoutMode::Floating } else { app.ui.layout }
}

pub(crate) fn shell_cards(app: &SolveApp, available: Rect) -> crate::ui_kit::Cards {
    if crate::render_workspace::active(app) {
        crate::ui_kit::StageBar::cards(available)
    } else {
        crate::ui_kit::Cards::new(available, shell_layout(app), reserved_height(app), WORKSPACE_WIDTH)
    }
}

/// The ribbon is folded to its tab row (`ribbon_tabs_only`, #10112). The Render workspace keeps
/// its own stage bar whole.
pub(crate) fn tabs_only(app: &SolveApp) -> bool {
    app.preferences.ribbon_tabs_only && !crate::render_workspace::active(app)
}

/// The height the ribbon keeps from the view: the tab row alone while it is folded, whose panels
/// then open over the view.
pub(crate) fn reserved_height(app: &SolveApp) -> f32 {
    if tabs_only(app) { TAB_ROW_HEIGHT + 1.0 } else { ribbon_height(app.preferences.ribbon_labels) }
}

/// Where one pass of the ribbon draws: the folded tab row, the whole ribbon, or the whole ribbon
/// opened over the view from the folded row.
#[derive(Clone, Copy)]
struct RibbonShape {
    floating: bool,
    cards: crate::ui_kit::Cards,
    r: Rect,
    ws: Rect,
    folded: bool,
}

pub fn toolbar(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    crate::workspace::sync_context(app);
    let layout = shell_layout(app);
    let floating = Container::for_layout(layout, Surface::Panel).floating();
    let folded = tabs_only(app);
    if !folded {
        app.ui.ribbon_open = false;
    }
    let available = ui.available_rect_before_wrap();
    let cards = shell_cards(app, available);
    let height = if floating { cards.bottom - available.top() } else { reserved_height(app) };
    let fill = if floating { Color32::TRANSPARENT } else { t.toolbar };
    let host = Container::for_layout(layout, Surface::Chrome);
    let ribbon = host.edge("sc_toolbar", Edge::Top);
    let frame = host.edge_frame(fill);
    let mut tab_click = None;
    let chrome = host.show_edge(ui, ribbon.exact_size(height).show_separator_line(false), frame, |ui| {
        let r = if floating { Rect::from_min_max(cards.workspace.min, cards.ribbon.max) } else { frame.outer_rect(ui.max_rect()) };
        let ws = if floating { cards.workspace } else { Rect::from_min_size(r.left_top(), vec2(WORKSPACE_WIDTH, height - 1.0)) };
        tab_click = ribbon_contents(app, ui, RibbonShape { floating, cards, r, ws, folded });
    });
    if floating {
        let frame = host.frame(ui.ctx(), t.toolbar, crate::frame::Side::Left);
        for card in [cards.workspace, cards.ribbon] {
            Container::paint_inset(ui.painter(), frame.widget_rect(card), frame);
        }
    } else {
        Container::paint_inset(ui.painter(), chrome.response.rect - frame.outer_margin, frame);
    }
    if folded && (app.ui.ribbon_open || tab_click.is_some()) {
        app.ui.ribbon_open = true;
        open_ribbon(app, ui, available, chrome.response.rect);
    }
}

/// The folded ribbon's tab, clicked: its panels open over the view, as the whole ribbon would
/// show them, until a command starts (`SolveApp::start` closes it), the pointer leaves them,
/// Escape is pressed or the open tab is clicked again (#10112).
fn open_ribbon(app: &mut SolveApp, ui: &mut egui::Ui, available: Rect, folded: Rect) {
    let t = Tokens::get();
    let layout = shell_layout(app);
    let floating = Container::for_layout(layout, Surface::Panel).floating();
    let full = ribbon_height(app.preferences.ribbon_labels);
    let cards = crate::ui_kit::Cards::new(available, layout, full, WORKSPACE_WIDTH);
    let r = if floating {
        Rect::from_min_max(cards.workspace.min, cards.ribbon.max)
    } else {
        Rect::from_min_size(folded.min, vec2(folded.width(), full))
    };
    let ws = if floating { cards.workspace } else { Rect::from_min_size(r.left_top(), vec2(WORKSPACE_WIDTH, full - 1.0)) };
    let host = Container::for_layout(layout, Surface::Chrome);
    let mut tab_click = None;
    host.area(egui::Id::unique("sc_ribbon_open")).fixed_pos(r.min).show(ui.ctx(), |ui| {
        ui.set_min_size(r.size());
        if !floating {
            let frame = host.frame(ui.ctx(), t.toolbar, crate::frame::Side::Left);
            Container::paint_frame(ui.painter(), frame, r);
        }
        tab_click = ribbon_contents(app, ui, RibbonShape { floating, cards, r, ws, folded: false });
        if tab_click.is_some() {
            // The other tab's panels are drawn now; the old tab's open drop-down is gone with them.
            ui.ctx().data_mut(|d| d.remove::<egui::Id>(egui::Id::unique("sc_panel_open")));
        }
        if floating {
            let frame = host.frame(ui.ctx(), t.toolbar, crate::frame::Side::Left);
            for card in [cards.workspace, cards.ribbon] {
                Container::paint_inset(ui.painter(), frame.widget_rect(card), frame);
            }
        }
    });
    crate::scenario::publish_rect("ribbon:open", r);
    let ctx = ui.ctx().clone();
    // A panel's drop-down, a menu or a dragged button keep the panels open while the pointer is
    // on them, away from the panels.
    let busy = ctx.data(|d| d.get_temp::<egui::Id>(egui::Id::unique("sc_panel_open")).is_some())
        || app.viewport.context_menu.is_some()
        || ribbon::dragging(&ctx);
    let left = ctx.input(|i| i.pointer.latest_pos()).is_none_or(|p| !r.expand(t.space_4).contains(p));
    let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if tab_click == Some(true) || (!busy && (left || escape)) {
        app.ui.ribbon_open = false;
        ctx.request_repaint();
    }
}

/// One ribbon button as SolidCraft sees it: the command pinned there, the one a click runs (a
/// family's chosen variant), and the component's data for it.
struct PanelButton {
    pinned: &'static solidcraft_engine::CommandSpec,
    selected: &'static solidcraft_engine::CommandSpec,
    /// The family whose first variant is pinned here: its variants' menu hangs under the button.
    family: Option<&'static crate::sketch_families::Family>,
    /// The commands the button stands for in its panel's drop-down.
    covers: &'static [&'static str],
    label: String,
    name: String,
    icon: &'static str,
    size: ButtonSize,
    enabled: bool,
    pressed: bool,
    exit: bool,
    persistent: bool,
    disabled: Option<String>,
    split_title: String,
    split_name: String,
}

impl PanelButton {
    fn ribbon(&self) -> ribbon::Button<'_> {
        ribbon::Button {
            id: self.pinned.id,
            run: self.selected.id,
            label: &self.label,
            name: &self.name,
            icon: self.icon,
            size: self.size,
            enabled: self.enabled,
            pressed: self.pressed,
            exit: self.exit,
            split: self.family.map(|f| ribbon::Split { key: f.label, title: &self.split_title, name: &self.split_name }),
            app_paints: self.persistent,
            covers: self.covers,
        }
    }
}

/// A row of a panel in the » menu.
struct PanelMoreRow {
    id: &'static str,
    label: &'static str,
    icon: &'static str,
    shortcut: String,
    enabled: bool,
    checked: Option<bool>,
    hint: Option<String>,
}

/// One panel of the tab as the ribbon component's group.
struct Panel {
    panel: &'static str,
    key: String,
    label: String,
    tip: String,
    popup_id: egui::Id,
    commands: Vec<&'static str>,
    icon: Option<&'static str>,
    enabled: bool,
    open: bool,
    large: Vec<PanelButton>,
    gallery: Vec<PanelButton>,
    more: Vec<PanelMoreRow>,
}

/// The tab's panels for one pass of the ribbon, as the component's data.
struct RibbonPanels {
    panels: Vec<Panel>,
    /// The last panel leaves the sketch or the edit.
    exit: bool,
    running: Option<String>,
}

impl RibbonPanels {
    fn new(app: &SolveApp, ui: &egui::Ui, tab: &str, lists: &[PanelList], painter: &egui::Painter, running: Option<String>) -> Self {
        let labels = app.preferences.ribbon_labels;
        let editing_in_place = app.edit_in_place.active.is_some();
        let open_menu = ui.ctx().data(|d| d.get_temp::<egui::Id>(egui::Id::unique("sc_panel_open")));
        let panels = lists
            .iter()
            .map(|(panel, cmds, shown)| {
                let panel: &'static str = panel;
                let sketch_create = tab == "SKETCH" && panel == "CREATE";
                let finish = (tab == "SKETCH" && panel == "FINISH SKETCH") || (tab == "MODEL" && panel == "FINISH EDITING");
                let in_gallery = |c: &&'static solidcraft_engine::CommandSpec| app.toolbar_custom.in_gallery(tab, panel, c.id);
                let button = |c: &'static solidcraft_engine::CommandSpec| {
                    // Only the family's default pin follows the part's choice. Explicitly pinned
                    // variants remain exact commands, including in customised toolbars.
                    let family = crate::sketch_families::family(c.id).filter(|f| sketch_create && f.commands.first().copied() == Some(c.id));
                    let selected = family.and_then(|f| app.preferences.sketch_variants.selected(f)).unwrap_or(c);
                    let info = crate::render_workspace::interactive_command_info(app, selected);
                    let persistent = persistent_available(app, selected.id);
                    let size = if labels {
                        let label = family.map_or_else(|| crate::workspace::ribbon_button_label(c), crate::workspace::ribbon_family_label);
                        ButtonSize { width: ribbon::button_width(painter, app.tr_label(label)), min_width: BUTTON_MIN_WIDTH }
                    } else {
                        let width = if family.is_some() { 50.0 } else { 40.0 };
                        ButtonSize { width, min_width: width }
                    };
                    let button_label = crate::workspace::ribbon_button_label(selected);
                    PanelButton {
                        pinned: c,
                        selected,
                        family,
                        covers: crate::sketch_families::family(c.id).filter(|_| sketch_create).map_or(&[], |f| f.commands),
                        label: app.tr_label(if button_label == selected.label { info.label } else { button_label }).to_string(),
                        name: app.tr_label(info.label).to_string(),
                        icon: if selected.id == "sketch.finish" { "finish" } else { info.icon },
                        size,
                        enabled: info.enabled,
                        pressed: (ribbon_shows_pressed(app, selected.id) && !(persistent && app.preferences.persistent_ribbon_pressed))
                            || (selected.id == "view.shots_strip" && app.preferences.render_shots_strip),
                        // The buttons that leave the sketch or the edit never show pressed; a
                        // tool pinned into their group does.
                        exit: finish
                            && matches!(selected.id, "sketch.finish" | "sketch.finish_auto_constrain" | "sketch.cancel" | "component.finish_edit"),
                        persistent,
                        disabled: info.disabled_reason,
                        split_title: family.map(|f| app.tr_label(crate::workspace::ribbon_family_label(f)).to_string()).unwrap_or_default(),
                        split_name: family.map(|f| app.tr_label(f.label).to_string()).unwrap_or_default(),
                    }
                };
                let cell = |c: &'static solidcraft_engine::CommandSpec| {
                    let info = crate::render_workspace::interactive_command_info(app, c);
                    PanelButton {
                        pinned: c,
                        selected: c,
                        family: None,
                        covers: &[],
                        label: String::new(),
                        name: app.tr_label(info.label).to_string(),
                        icon: info.icon,
                        size: ButtonSize { width: 0.0, min_width: 0.0 },
                        enabled: info.enabled,
                        pressed: ribbon_shows_pressed(app, c.id),
                        exit: false,
                        persistent: false,
                        disabled: info.disabled_reason,
                        split_title: String::new(),
                        split_name: String::new(),
                    }
                };
                let more = menu_commands(cmds, shown)
                    .into_iter()
                    .map(|c| {
                        let info = crate::render_workspace::interactive_command_info(app, c);
                        let checked =
                            (persistent_available(app, c.id) && app.preferences.persistent_ribbon_pressed).then(|| persistent_pressed(app, c.id));
                        PanelMoreRow {
                            id: c.id,
                            label: info.label,
                            icon: info.icon,
                            shortcut: crate::keymap::display_for(app, c.id).unwrap_or_default(),
                            enabled: info.enabled,
                            checked,
                            hint: persistent_hint(app, c, info.enabled),
                        }
                    })
                    .collect();
                // Per tab: panels of the same name on other tabs (MODIFY, CREATE…) list other
                // commands, and the drop-down's area remembers its size by this id (#874).
                let popup_id = ui.scope_id().with(("panel_menu", tab, panel));
                Panel {
                    panel,
                    key: format!("{tab}:{panel}"),
                    label: if finish { app.tr("ui.ribbon_exit").to_string() } else { ribbon_label(app, panel) },
                    tip: format!("{} ({})", app.tr_label(panel), shown.len().max(cmds.len())),
                    popup_id,
                    commands: cmds.iter().map(|c| c.id).collect(),
                    icon: shown.first().or(cmds.first()).map(|c| c.info(&app.session).icon),
                    enabled: !cmds.is_empty() || panel == "SELECT",
                    open: open_menu == Some(popup_id),
                    large: shown.iter().copied().filter(|c| !in_gallery(c)).map(button).collect(),
                    gallery: shown.iter().copied().filter(in_gallery).map(cell).collect(),
                    more,
                }
            })
            .collect::<Vec<_>>();
        // The Sketch exit group follows the last sketch panel (Finish Edit the last panel while
        // editing in place).
        let exit = (tab == "SKETCH" || (tab == "MODEL" && editing_in_place))
            && panels.last().is_some_and(|p| matches!(p.panel, "FINISH SKETCH" | "FINISH EDITING"));
        RibbonPanels { panels, exit, running: if app.preferences.ribbon_reveal { running } else { None } }
    }

    fn row<'a>(
        &'a self,
        accent: solidcraft_theme::TabAccent,
        labels: bool,
        reveal: Option<ribbon::Reveal<'a>>,
        more_tip: &'a str,
    ) -> ribbon::Row<'a> {
        ribbon::Row {
            groups: self
                .panels
                .iter()
                .map(|p| ribbon::Group {
                    key: &p.key,
                    name: p.panel,
                    label: &p.label,
                    tip: &p.tip,
                    buttons: p.large.iter().map(PanelButton::ribbon).collect(),
                    cells: p.gallery.iter().map(PanelButton::ribbon).collect(),
                    commands: p.commands.clone(),
                    icon: p.icon,
                    enabled: p.enabled,
                    open: p.open,
                    running: self.running.as_deref(),
                    more: p
                        .more
                        .iter()
                        .map(|m| ribbon::MoreRow {
                            id: m.id,
                            label: m.label,
                            icon: m.icon,
                            shortcut: &m.shortcut,
                            enabled: m.enabled,
                            checked: m.checked,
                            hint: m.hint.as_deref(),
                        })
                        .collect(),
                })
                .collect(),
            labels,
            accent,
            exit: self.exit,
            reveal,
            more_tip,
            handles: RIBBON_HANDLES,
        }
    }

    /// The id of panel `group`'s drop-down.
    fn popup_id(&self, group: usize) -> Option<egui::Id> {
        self.panels.get(group).map(|p| p.popup_id)
    }

    /// The panel button the component drew as `drawn`.
    fn panel_button(&self, drawn: &ribbon::Drawn) -> Option<&PanelButton> {
        let panel = self.panels.get(drawn.group)?;
        if drawn.cell { panel.gallery.get(drawn.index) } else { panel.large.get(drawn.index) }
    }
}

/// Where SolidCraft's ribbon publishes its parts (scripts and scenarios use these names).
const RIBBON_HANDLES: ribbon::Handles<'static> = ribbon::Handles {
    button: &["toolbar:", "button:"],
    button_rect: "button:",
    split: "family:",
    cell: &["toolbar:", "button:", "ribbon:constraint:"],
    collapsed: "collapsed:",
    section: "panel:",
    section_target: "ribbon:section:",
    group: "panel:",
    running: "ribbon:running_section:",
    reveal: "ribbon:reveal",
    more: "toolbar:overflow",
    more_row: "toolbar:",
    collapsed_count: "toolbar_collapsed",
    more_count: "toolbar_more",
    trimmed_count: "toolbar_trimmed",
    drag: "toolbar_drag",
    drag_remove: "toolbar_drag:remove",
};

/// One panel of a tab: its name, every command and the buttons it shows.
type PanelList = (&'static str, Vec<&'static solidcraft_engine::CommandSpec>, Vec<&'static solidcraft_engine::CommandSpec>);

/// One pass of the ribbon in `shape`. Returns `Some(already shown)` when a tab was clicked.
fn ribbon_contents(app: &mut SolveApp, ui: &mut egui::Ui, shape: RibbonShape) -> Option<bool> {
    let t = Tokens::get();
    let layout = shell_layout(app);
    let host = Container::for_layout(layout, Surface::Chrome);
    let labels = app.preferences.ribbon_labels;
    let group_height = if labels { GROUP_HEIGHT } else { ICON_GROUP_HEIGHT };
    let RibbonShape { floating, cards, r, ws, folded } = shape;
    {
        let ribbon_left = if floating { cards.ribbon.left() } else { ws.right() };
        if floating {
            for card in [ws, cards.ribbon] {
                let fill =
                    if crate::render_workspace::active(app) && card == ws { solidcraft_theme::TabAccent::current(ui.ctx()).soft } else { t.toolbar };
                let frame = host.frame(ui.ctx(), fill, crate::frame::Side::Left);
                host.paint_background(ui.painter(), frame, card);
                // Blank card space owns clicks as well as its buttons.
                ui.interact(card, ui.scope_id().with(("shell_card", card.left().to_bits())), Sense::click_and_drag());
            }
            if !crate::render_workspace::active(app) {
                crate::scenario::publish_rect("ribbon:card", cards.ribbon);
            }
        }
        app.shell.cards = vec![ws, Rect::from_min_max(pos2(ribbon_left, r.top()), r.max)];
        if crate::render_workspace::active(app) {
            ui.set_clip_rect(ui.clip_rect().intersect(r));
        }
        let painter = ui.painter().clone();
        let bg = ui.interact(if floating { cards.ribbon } else { r }, ui.scope_id().with("toolbar_bg"), Sense::click());
        if bg.secondary_clicked() {
            crate::context_menu::open_for(app, bg.interact_pointer_pos().unwrap_or(r.left_bottom()), crate::context_menu::Target::Toolbar);
        }
        let render = crate::render_workspace::active(app);
        let switcher = app_bar::Switcher {
            name: app.tr(if render { "ui.render" } else { "ui.design" }),
            caption: app.tr("ui.ribbon_workspace"),
            icon: if render { "ws_render" } else { "ws_design" },
            tip: app.tr("ui.change_workspace"),
            open: app.viewport.context_menu.is_some() && app.menu.target == crate::context_menu::Target::Workspace,
            floating,
            labels,
            folded,
            stage: render,
            accent: if render { solidcraft_theme::TabAccent::current(ui.ctx()).color } else { t.accent },
            handle_rect: "workspace_switcher",
            handle_target: "ribbon:workspace",
        };
        if app_bar::switcher(ui, &switcher, ws) {
            crate::context_menu::toggle_for(app, ws, ws.left_bottom(), crate::context_menu::Target::Workspace);
        }
        crate::scenario::publish_rect("toolbar", r);
        if render {
            crate::render_ui::stage_bar(app, ui, cards.ribbon);
            return None;
        }
        let tab_click = ribbon_tabs(app, ui, Rect::from_min_max(pos2(ribbon_left, r.top()), pos2(r.right(), r.top() + TAB_ROW_HEIGHT)));
        if folded {
            crate::scenario::publish_rect("ribbon:folded", r);
            return tab_click;
        }
        // Panels.
        let tab = app.ui.tab.clone();
        // The command whose tool or dialog is open right now, shown pressed in the toolbar.
        let running = running_command(app);
        // The command the search points out (#7287): a preview while a result is hovered, or the
        // short pulse after starting one.
        let now = ui.input(|i| i.time);
        let reveal = app.sbox.reveal.clone().filter(|r| r.preview || now - r.since < crate::shortcut_box::REVEAL_PULSE);
        let pulse = reveal.as_ref().map_or(0.0, |r| r.strength(now, ui.ctx().global_style().animation_time > 0.0));
        if reveal.is_some() {
            ui.ctx().request_repaint();
        }
        let mut persistent_paints = Vec::new();
        let top = r.top() + TAB_ROW_HEIGHT;
        let mut drop: Option<(String, &'static str, Option<String>)> = None;
        let mut menu: Option<(egui::Pos2, &'static str)> = None;
        // A drop-down row's pin icon clicked: pin or unpin that command.
        let mut toggle: Option<&'static str> = None;
        // The open drop-down (a button dropped on it leaves the toolbar).
        let mut popup_rect: Option<Rect> = None;
        // Search can draw a preview of its own. Only the open panel menu follows the pointer.
        let mut open_panel_menu = None;
        let mut hovered_panel_menu = None;
        let follow_panel_hover = app.preferences.ribbon_menus_follow_hover
            && ui.input(|i| {
                !i.pointer.any_down()
                    && i.events.iter().any(|e| matches!(e, egui::Event::PointerMoved(_) | egui::Event::PointerGone))
                    && !i.events.iter().any(|e| matches!(e, egui::Event::Touch { .. }))
            })
            && !ribbon::dragging(ui.ctx());
        // Every panel with all its buttons (the user's pins have no cap); a toolbar too narrow
        // for them narrows the buttons, then trims panels into their drop-down and the » menu.
        let editing_in_place = app.edit_in_place.active.is_some();
        let lists: Vec<_> = crate::toolbar_custom::panel_lists(&tab)
            .into_iter()
            .filter(|(panel, _, _)| editing_in_place || !(tab == "MODEL" && *panel == "FINISH EDITING"))
            .map(|(panel, mut cmds, promote)| {
                let promote = if tab == "SKETCH" && panel == "CREATE" && !app.preferences.sketch_construction_lines {
                    promote.saturating_sub(1)
                } else {
                    promote
                };
                if !app.preferences.sketch_construction_lines {
                    cmds.retain(|c| solidcraft_engine::cmd::construction_line_mode(c.id).is_none());
                }
                let mut shown = crate::toolbar_custom::buttons(app, &tab, panel, &cmds, promote);
                // Gallery cells are painted together after the large buttons, in reading order.
                if tab == "SKETCH" && panel == "CONSTRAIN" {
                    shown.sort_by_key(|c| {
                        if app.toolbar_custom.in_gallery(&tab, panel, c.id) {
                            crate::workspace::CONSTRAINT_GALLERY.iter().position(|id| *id == c.id).map(|i| i + 1).unwrap_or(0)
                        } else {
                            0
                        }
                    });
                }
                (panel, cmds, shown)
            })
            .collect();
        let right = r.right();
        if crate::workspace::visible_tabs(app).is_empty() {
            // Every tab is hidden: the ribbon says how to bring them back.
            let at = pos2((ribbon_left + right) / 2.0, top + group_height / 2.0);
            ribbon::text(ui, at, app.tr("ui.toolbar_tabs_hidden"), t.font_body, t.text_dim, false);
            crate::scenario::publish_handle("ribbon:tabs_hidden", at);
        }
        let panels = RibbonPanels::new(app, ui, &tab, &lists, &painter, running.clone());
        let more_tip = solidcraft_ui_text::tr_label("More toolbar panels");
        let accent = crate::workspace::tab_accent(&t, &tab);
        let row = panels.row(accent, labels, reveal.as_ref().map(|r| ribbon::Reveal { id: r.id, preview: r.preview, strength: pulse }), more_tip);
        let mut out = {
            let mut more_rows = |ui: &mut egui::Ui, group: usize| {
                let Some((panel, cmds, shown)) = lists.get(group) else { return false };
                if tab == "SKETCH" && *panel == "CREATE" {
                    let cmds = menu_commands(cmds, shown);
                    sketch_create_menu(app, ui, &cmds, dropdown_width(app, ui, &cmds), &mut menu, &mut toggle);
                    return true;
                }
                if *panel == "SELECT" {
                    selection_filter(app, ui);
                }
                false
            };
            ribbon::show(ui, &row, Rect::from_min_max(pos2(ribbon_left, top), r.max), &mut more_rows)
        };
        // The app's part of each button: its hover text, drags, the search's preview, deferred
        // persistent paint and a family's variants menu.
        for d in &mut out.buttons {
            let deferred = d.deferred.take();
            let Some(b) = panels.panel_button(d) else { continue };
            let resp = &d.response;
            if let Some(r) = reveal.as_ref().filter(|r| r.preview) {
                if d.cell {
                    if r.id == b.pinned.id {
                        reveal_tooltip(app, ui, d.whole, b.pinned, egui::RectAlign::BOTTOM_START);
                    }
                } else if (r.id == b.pinned.id || r.id == b.selected.id || b.family.is_some_and(|f| f.commands.contains(&r.id)))
                    && let Some(command) = solidcraft_engine::find_command(r.id)
                {
                    reveal_tooltip(app, ui, d.whole, command, egui::RectAlign::BOTTOM_START);
                }
            }
            if let Some(deferred) = deferred {
                persistent_paints.push(PersistentRibbonPaint {
                    command: b.selected,
                    deferred,
                    label: labels.then(|| persistent_button_name(app, b.selected)),
                    enabled: ui.is_enabled() && b.enabled,
                    disabled: b.disabled.clone(),
                });
            } else {
                let _ = ribbon_tooltip(app, resp.clone(), b.selected, b.disabled.as_deref());
            }
            if let (Some(arrow), Some(f)) = (&d.split, b.family) {
                Container::new(ui.ctx(), Surface::Popup).menu(arrow).show(|ui| {
                    let variants: Vec<_> = f.commands.iter().filter_map(|id| solidcraft_engine::find_command(id)).collect();
                    dropdown_keys(app, ui, &variants);
                    let width = dropdown_width(app, ui, &variants);
                    sketch_menu_rows(app, ui, &variants, width, &mut menu, &mut toggle);
                });
            }
        }
        // What was chosen on the row, in order.
        let mut opened = Vec::new();
        for action in std::mem::take(&mut out.actions) {
            match action {
                ribbon::Action::Run { id, rect, place } => {
                    let Some(command) = solidcraft_engine::find_command(&id) else { continue };
                    match place {
                        ribbon::Place::Button => {
                            if persistent_available(app, command.id)
                                && app.preferences.persistent_ribbon_pressed
                                && let Some(d) = out.buttons.iter().find(|d| !d.cell && d.whole == rect)
                            {
                                d.response.request_focus();
                            }
                            if !end_running_tool(app, command.id) {
                                app.start_pinned(command.id, Some(rect));
                            }
                        }
                        ribbon::Place::Cell => {
                            if !end_running_tool(app, command.id) {
                                app.start_at(command.id, Some(rect));
                            }
                        }
                        ribbon::Place::More => app.start_at(command.id, Some(rect)),
                    }
                }
                ribbon::Action::Menu { id, at } => {
                    if let Some(command) = solidcraft_engine::find_command(&id) {
                        menu = Some((at, command.id));
                    }
                }
                ribbon::Action::Section { group } => {
                    let Some(popup_id) = panels.popup_id(group) else { continue };
                    ui.ctx().data_mut(|d| {
                        let key = egui::Id::unique("sc_panel_open");
                        if d.get_temp::<egui::Id>(key) == Some(popup_id) {
                            d.remove::<egui::Id>(key);
                        } else {
                            d.insert_temp(key, popup_id);
                        }
                    });
                }
                ribbon::Action::Drop { id, group, before } => {
                    if let Some((panel, _, _)) = lists.get(group) {
                        drop = Some((id, *panel, before));
                    }
                }
                ribbon::Action::Open { group } => {
                    let Some(popup_id) = panels.popup_id(group) else { continue };
                    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("sc_panel_open"), popup_id));
                    opened.push(group);
                }
            }
        }
        // Each panel's drop-down, and a button dropped on its free space.
        for g in &out.groups {
            let (Some((panel, cmds, shown)), Some(data)) = (lists.get(g.group), panels.panels.get(g.group)) else { continue };
            let panel: &&str = panel;
            let sketch_create = tab == "SKETCH" && *panel == "CREATE";
            let popup_id = data.popup_id;
            let (collapsed, trimmed, lr, lresp) = (g.collapsed, g.trimmed, g.label, &g.label_response);
            let opened = opened.contains(&g.group);
            if follow_panel_hover && g.hot {
                hovered_panel_menu = Some(popup_id);
            }
            // The commands the ribbon shows as buttons now (a narrow window shows fewer), which
            // `ribbon_menus_skip_pinned` leaves out of the drop-down.
            let on_ribbon: Vec<&str> =
                data.large.iter().take(g.buttons).map(|b| b.pinned.id).chain(data.gallery.iter().filter(|_| g.cells).map(|b| b.pinned.id)).collect();
            let open = ui.ctx().data(|d| d.get_temp::<egui::Id>(egui::Id::unique("sc_panel_open"))) == Some(popup_id);
            // A hovered search result in this drop-down opens it on its row, under the search sheet.
            let previewed = g.previewed && !open;
            if open || previewed {
                let cmds = if collapsed || trimmed { menu_commands(cmds, shown) } else { cmds.clone() };
                let cmds = if app.preferences.ribbon_menus_skip_pinned { skip_on_ribbon(cmds, &on_ribbon) } else { cmds };
                let area = Container::new(ui.ctx(), Surface::Popup)
                    .area(popup_id)
                    .fixed_pos(lr.left_bottom())
                    .order(if open { egui::Order::Foreground } else { egui::Order::Middle })
                    .show(ui.ctx(), |ui| {
                        crate::ui_kit::Menu::show_frame(ui, crate::ui_kit::Menu::frame(), |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.set_min_width(200.0);
                            if *panel == "SELECT" {
                                selection_filter(app, ui);
                            } else if cmds.is_empty() {
                                ui.label(egui::RichText::new(solidcraft_ui_text::tr_label("Not available yet")).color(t.text_dim));
                            }
                            if sketch_create && app.preferences.ribbon_icon_grid && !cmds.is_empty() {
                                let mut paints = Vec::new();
                                let action = dropdown_grid(app, ui, &tab, &sketch_create_grouped(&cmds), &[], &mut paints);
                                grid_action(app, ui, action, &mut menu, &mut toggle);
                                paint_persistent_ribbon(app, ui, paints, &tab, "ribbon_grid");
                                return;
                            }
                            if sketch_create {
                                // One width for every row and the footer, as in the other panels.
                                let row_w = dropdown_width(app, ui, &cmds);
                                ui.set_width(row_w);
                                sketch_create_menu(app, ui, &cmds, row_w, &mut menu, &mut toggle);
                                ribbon::section_footer(ui, solidcraft_ui_text::tr("ui.ribbon_pin_footer"));
                                return;
                            }
                            let grouped = if *panel == "CONSTRUCT" && !app.preferences.construct_menu_groups {
                                cmds.iter().map(|c| (None, *c)).collect()
                            } else {
                                crate::workspace::grouped_commands(&tab, panel, &cmds)
                            };
                            if app.preferences.ribbon_icon_grid && !grouped.is_empty() {
                                let mut paints = Vec::new();
                                let action = dropdown_grid(app, ui, &tab, &grouped, &crate::workspace::ungrouped(&tab, panel, &cmds), &mut paints);
                                grid_action(app, ui, action, &mut menu, &mut toggle);
                                // Paint in this popup's layer, after its command has changed the session.
                                paint_persistent_ribbon(app, ui, paints, &tab, "ribbon_grid");
                                return;
                            }
                            let cmds: Vec<_> = grouped.iter().map(|(_, c)| *c).collect();
                            let row_w = dropdown_width(app, ui, &cmds);
                            ui.set_width(row_w);
                            // Up and Down move the keyboard focus through the enabled rows (from none
                            // focused: the first or the last), instead of egui's own spatial move.
                            let (down, up) = ui.input(|i| (i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::ArrowUp)));
                            if down || up {
                                let enabled: Vec<&str> = cmds.iter().filter(|c| c.info(&app.session).enabled).map(|c| c.id).collect();
                                let focused = ui.ctx().memory(|m| m.focused());
                                let at = enabled.iter().position(|id| focused == Some(menu_style(app).focus_id(id)));
                                let n = enabled.len();
                                let next = match (at, down) {
                                    (None, true) => Some(0),
                                    (None, false) => n.checked_sub(1),
                                    (Some(i), true) => Some((i + 1) % n.max(1)),
                                    (Some(i), false) => Some((i + n.max(1) - 1) % n.max(1)),
                                };
                                if let Some(id) = next.and_then(|i| enabled.get(i)) {
                                    ui.ctx().memory_mut(|m| {
                                        m.request_focus(menu_style(app).focus_id(id));
                                        m.move_focus(egui::FocusDirection::None);
                                    });
                                }
                            }
                            // The rows scroll when the drop-down would run past the window's bottom.
                            // `min_scrolled_height` lets the rows grow past the size the area kept from
                            // an earlier, shorter list (#874); they still shrink to fit their content.
                            let room = (ui.ctx().content_rect().bottom() - ui.cursor().top() - 64.0).max(ROW_H * 3.0);
                            egui::ScrollArea::vertical().id_salt("panel_menu_rows").max_height(room).min_scrolled_height(room).show(ui, |ui| {
                                for (index, (heading, c)) in grouped.iter().enumerate() {
                                    if let Some(heading) = heading {
                                        if *panel == "CONSTRUCT" && index > 0 {
                                            ui.separator();
                                        }
                                        ribbon::section_heading(ui, &group_heading_text(heading), row_w);
                                    }
                                    let pinned = crate::toolbar_custom::pinned_on(app, &tab, c.id).is_some();
                                    let revealed = reveal.as_ref().is_some_and(|r| r.id == c.id);
                                    match dropdown_row(app, ui, c, row_w, pinned, revealed.then_some(pulse)) {
                                        None => {}
                                        Some(ribbon::MenuAction::Menu(at)) => menu = Some((at, c.id)),
                                        Some(ribbon::MenuAction::TogglePin) => toggle = Some(c.id),
                                        Some(ribbon::MenuAction::Run(rect)) => {
                                            app.start_at(c.id, Some(rect));
                                            ui.ctx().data_mut(|d| d.remove::<egui::Id>(egui::Id::unique("sc_panel_open")));
                                        }
                                    }
                                }
                            });
                            ribbon::section_footer(ui, solidcraft_ui_text::tr("ui.ribbon_pin_footer"));
                        });
                    });
                popup_rect = Some(area.response.rect);
                if open {
                    open_panel_menu = Some((popup_id, area.response.rect));
                }
                crate::scenario::publish_rect("toolbar_dropdown", area.response.rect);
                // A drag out of the drop-down ends with a release elsewhere: not a click elsewhere.
                let dragging = ribbon::dragging(ui.ctx());
                if open
                    && ((area.response.clicked_elsewhere() && !lresp.clicked() && !opened && !dragging)
                        || ui.input(|i| i.key_pressed(egui::Key::Escape)))
                {
                    ui.ctx().data_mut(|d| d.remove::<egui::Id>(egui::Id::unique("sc_panel_open")));
                }
            }
        }

        // Resolve persistent paint only after large buttons, dropdowns and overflow have
        // dispatched their actions. No already-painted old analysis can survive replacement.
        paint_persistent_ribbon(app, ui, persistent_paints, &tab, "ribbon");

        if follow_panel_hover {
            let ribbon = Rect::from_min_max(pos2(ribbon_left, r.top()), r.max);
            let pointer = ui.input(|i| i.pointer.hover_pos());
            let changed = ui.ctx().data_mut(|d| {
                let key = egui::Id::unique("sc_panel_open");
                let Some(open) = d.get_temp::<egui::Id>(key) else { return false };
                let menu = open_panel_menu.filter(|(id, _)| *id == open).map(|(_, rect)| rect);
                let stays_open =
                    pointer.zip(menu).is_some_and(|(p, menu)| ribbon.expand(t.space_4).contains(p) || menu.expand(t.space_4).contains(p));
                if !stays_open {
                    d.remove::<egui::Id>(key);
                    true
                } else if let Some(hovered) = hovered_panel_menu.filter(|hovered| *hovered != open) {
                    // Change the id after drawing all panels, so only one menu is drawn per pass.
                    d.insert_temp(key, hovered);
                    true
                } else {
                    false
                }
            });
            if changed {
                ui.ctx().request_repaint();
            }
        }

        // Dragging a command: its icon follows the pointer; a toolbar button dragged off the
        // toolbar (or into the open drop-down) shows it will be removed.
        let icon_of = |id: &str| solidcraft_engine::find_command(id).map(|c| c.info(&app.session).icon.to_string());
        if let Some(id) = ribbon::drag_end(ui, r, popup_rect, drop.is_some(), &icon_of, &RIBBON_HANDLES) {
            crate::toolbar_custom::remove(app, &tab, &id);
        }
        if let Some((id, panel, before)) = drop {
            crate::toolbar_custom::reorder(app, &tab, &id, panel, before.as_deref());
        }
        if let Some(id) = toggle {
            crate::toolbar_custom::toggle(app, &tab, id);
        }
        if let Some((at, id)) = menu {
            ui.ctx().data_mut(|d| d.remove::<egui::Id>(egui::Id::unique("sc_panel_open")));
            crate::context_menu::open_for(app, at, crate::context_menu::Target::ToolbarCommand { id: id.to_string() });
        }
        tab_click
    }
}

/// A quick-access button being dragged along the bar (#10223).
#[derive(Clone)]
struct QuickAccessDrag(String);

/// The quick-access bar (#10223): the commands added from a command's menu, after Save, Undo and
/// Redo, each with its own icon. A click starts the command as its ribbon button does, a
/// right-click opens the command's menu (Remove from Quick Access), and a drag along the bar
/// reorders the buttons. They start at `x`, `step` apart ([`app_bar::BarOut::quick`]).
fn quick_access_bar(app: &mut SolveApp, ui: &mut egui::Ui, bar: Rect, mut x: f32, step: f32, shown: bool) {
    let t = Tokens::get();
    let ids: Vec<&'static solidcraft_engine::CommandSpec> =
        app.toolbar_custom.quick_access.iter().filter_map(|id| solidcraft_engine::find_command(id)).collect();
    if ids.is_empty() {
        return;
    }
    let first = x;
    let mut buttons = Vec::with_capacity(ids.len());
    let mut start = None;
    let mut menu = None;
    for c in &ids {
        let br = Rect::from_center_size(pos2(x, bar.center().y), vec2(32.0, 32.0));
        x += step;
        let info = c.info(&app.session);
        let enabled = shown && info.enabled;
        let resp = ui.interact(br, ui.scope_id().with(("quick_access", c.id)), Sense::click_and_drag());
        let label = app.tr_label(info.label).to_string();
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &label));
        crate::scenario::publish_target(&format!("appbar:quick:{}", c.id), br);
        if enabled && (resp.hovered() || resp.has_focus()) {
            ui.painter().rect_filled(br.shrink(3.0), t.radius_sm, t.app_bar_hover_button);
        }
        icons::paint_on(ui.painter(), Rect::from_center_size(br.center(), vec2(20.0, 20.0)), info.icon, Some(true), if enabled { 1.0 } else { 0.35 });
        if resp.drag_started() {
            egui::DragAndDrop::set_payload(ui.ctx(), QuickAccessDrag(c.id.to_string()));
        }
        if resp.secondary_clicked() {
            menu = Some((resp.interact_pointer_pos().unwrap_or(br.left_bottom()), c.id));
        }
        let tip = match crate::keymap::display_for(app, c.id) {
            Some(key) if !key.is_empty() => format!("{label} ({key})"),
            _ => label.clone(),
        };
        let tip = if shown { tip } else { solidcraft_ui_text::format("ui.needs_design", &[("command", &label)]) };
        if resp.backing_on_hover_text(tip).clicked() && enabled {
            start = Some((c.id, br));
        }
        buttons.push((c.id, br));
    }
    let span = Rect::from_min_max(pos2(first - step / 2.0, bar.top()), pos2(x - step / 2.0, bar.bottom()));
    crate::scenario::publish_rect("appbar:quick_access", span);
    // Dragged along the bar: the line shows where it goes; let go and it moves there.
    if let Some(drag) = egui::DragAndDrop::payload::<QuickAccessDrag>(ui.ctx())
        && let Some(p) = ui.input(|i| i.pointer.latest_pos())
    {
        let before = buttons.iter().find(|(_, br)| p.x < br.center().x).map(|(id, _)| *id);
        let line = before.and_then(|b| buttons.iter().find(|(id, _)| *id == b)).map_or(span.right(), |(_, br)| br.left());
        if span.expand2(vec2(step, 0.0)).contains(p) {
            ui_kit::insertion_marker(ui.painter(), line, bar.top() + 6.0, bar.bottom() - 6.0);
        }
        if ui.input(|i| i.pointer.any_released()) {
            egui::DragAndDrop::clear_payload(ui.ctx());
            if span.expand2(vec2(step, 0.0)).contains(p) {
                crate::toolbar_custom::quick_move(app, &drag.0, before);
            }
        }
    }
    if let Some((id, br)) = start {
        // The exact pinned command, without replacing its family's remembered ribbon choice.
        app.start_pinned(id, Some(br));
    }
    if let Some((at, id)) = menu {
        crate::context_menu::open_for(app, at, crate::context_menu::Target::ToolbarCommand { id: id.to_string() });
    }
}

/// The chat panel's compact entry point. State stays in agent_chat; the bar only paints it.
fn assistant_button(app: &mut SolveApp, ui: &mut egui::Ui, at: Rect) {
    use crate::agent_chat::AssistantMark;
    let resp = ui.interact(at, ui.scope_id().with("assistant_button"), Sense::click());
    crate::scenario::publish_target("titlebar:assistant", at);
    let mark = crate::agent_chat::assistant_mark(app);
    let tip = crate::agent_chat::assistant_tip(app, mark);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), &tip));
    let state = match mark {
        AssistantMark::Quiet => crate::ui_kit::AssistantEntryState::Quiet,
        AssistantMark::Ready => crate::ui_kit::AssistantEntryState::Ready,
        AssistantMark::Busy => crate::ui_kit::AssistantEntryState::Busy,
        AssistantMark::NeedsAuth | AssistantMark::NoDesign => crate::ui_kit::AssistantEntryState::NeedsAuth,
        AssistantMark::Stopped => crate::ui_kit::AssistantEntryState::Stopped,
        AssistantMark::Waiting(count) => crate::ui_kit::AssistantEntryState::Waiting(count),
    };
    crate::ui_kit::assistant_entry(ui, &resp, state, app.agent_chat.open, ui.ctx().global_style().animation_time > 0.0);
    if resp.backing_on_hover_text(tip).clicked() {
        crate::agent_chat::toggle(app);
    }
}

/// SolidCraft's group menus: the search's accent, its pin tips and where rows publish.
fn menu_style(app: &SolveApp) -> ribbon::MenuStyle<'static> {
    ribbon::MenuStyle {
        pin_tip: solidcraft_ui_text::tr_label("Pin to Toolbar"),
        unpin_tip: solidcraft_ui_text::tr_label("Remove from Toolbar"),
        accent: crate::workspace::tab_accent(&Tokens::get(), &app.ui.tab),
        focus: egui::Id::unique("dropdown_row"),
        entry: &["toolbar:", "dropdown:"],
        pin: "dropdown_pin:",
        hot: "dropdown_hot:",
        reveal: "ribbon:reveal",
    }
}

/// Command `c` as a menu row: its label, shortcut, check mark and hover text.
struct RowData {
    label: String,
    shortcut: String,
    checked: Option<bool>,
    enabled: bool,
    icon: &'static str,
    tip: Option<String>,
    help: Option<String>,
    hint: bool,
}

fn dropdown_checkable(app: &SolveApp, id: &str) -> bool {
    id == "view.shots_strip" || (persistent_available(app, id) && app.preferences.persistent_ribbon_pressed)
}

impl RowData {
    fn new(app: &SolveApp, c: &'static solidcraft_engine::CommandSpec) -> Self {
        let info = crate::render_workspace::interactive_command_info(app, c);
        let label = app.tr_label(info.label).to_string();
        let checked = if c.id == "view.shots_strip" {
            Some(app.preferences.render_shots_strip)
        } else {
            dropdown_checkable(app, c.id).then(|| persistent_pressed(app, c.id))
        };
        let shortcut = crate::keymap::display_for(app, c.id).unwrap_or_default();
        // A disabled row says why; a documented command also shows its help (#2758).
        let reason = info.disabled_reason.as_deref().filter(|_| !info.enabled).map(|reason| app.tr_label(reason));
        let (tip, help, hint) = match persistent_hint(app, c, info.enabled) {
            Some(hint) => (Some(hint), None, true),
            None => (
                crate::command_help::row_tip(c.id, &label, Some(shortcut.as_str()).filter(|k| !k.is_empty()), reason),
                crate::command_help::help(c.id),
                false,
            ),
        };
        RowData { label, shortcut, checked, enabled: info.enabled, icon: info.icon, tip, help, hint }
    }

    fn entry<'a>(&'a self, c: &'static solidcraft_engine::CommandSpec, pinned: bool) -> ribbon::MenuEntry<'a> {
        ribbon::MenuEntry {
            id: c.id,
            label: &self.label,
            icon: self.icon,
            shortcut: &self.shortcut,
            enabled: self.enabled,
            checked: self.checked,
            pinned,
            tip: self
                .tip
                .as_deref()
                .map(|text| if self.hint { ribbon::RowTip::Plain(text) } else { ribbon::RowTip::Help { text, help: self.help.as_deref() } }),
            app_paints: false,
        }
    }
}

/// One command row of a panel drop-down ([`ribbon::menu_row`]); every row drags onto the
/// toolbar to pin it there. `revealed` is the pulse strength when the command search points
/// this row out (#7287).
fn dropdown_row(
    app: &SolveApp,
    ui: &mut egui::Ui,
    c: &'static solidcraft_engine::CommandSpec,
    width: f32,
    pinned: bool,
    revealed: Option<f32>,
) -> Option<ribbon::MenuAction> {
    let row = RowData::new(app, c);
    let (action, resp) = ribbon::menu_row(ui, &row.entry(c, pinned), width, revealed, &menu_style(app));
    if revealed.is_some() {
        reveal_tooltip(app, ui, resp.rect, c, egui::RectAlign::RIGHT_START);
    }
    action
}

/// Up and Down through these commands' enabled rows ([`ribbon::list_keys`]).
fn dropdown_keys(app: &SolveApp, ui: &egui::Ui, cmds: &[&'static solidcraft_engine::CommandSpec]) {
    let enabled: Vec<&str> = cmds.iter().filter(|c| crate::render_workspace::interactive_command_info(app, c).enabled).map(|c| c.id).collect();
    ribbon::list_keys(ui, &enabled, &menu_style(app));
}

/// How wide a panel drop-down's rows are: the widest label and shortcut, at least 200.
fn dropdown_width(app: &SolveApp, ui: &egui::Ui, cmds: &[&'static solidcraft_engine::CommandSpec]) -> f32 {
    let rows: Vec<_> = cmds
        .iter()
        .map(|c| {
            let info = c.info(&app.session);
            (
                app.tr_label(info.label).to_string(),
                crate::keymap::display_for(app, c.id).unwrap_or_default(),
                info.icon,
                dropdown_checkable(app, c.id),
            )
        })
        .collect();
    ribbon::menu_width(ui, rows.iter().map(|(label, key, icon, checkable)| (label.as_str(), key.as_str(), *icon, *checkable)))
}

/// A panel drop-down drawn as a grid of large icons (the `ribbon_icon_grid` preference, #4739):
/// the same commands, headings and actions as the list, the names in the tooltips. Returns the
/// command acted on, and how.
fn dropdown_grid(
    app: &SolveApp,
    ui: &mut egui::Ui,
    tab: &str,
    grouped: &[(Option<&'static str>, &'static solidcraft_engine::CommandSpec)],
    loose: &[&str],
    paints: &mut Vec<PersistentRibbonPaint>,
) -> Option<(&'static str, ribbon::MenuAction)> {
    let labels: Vec<String> = grouped.iter().map(|(_, c)| app.tr_label(c.label).to_string()).collect();
    let headings: Vec<Option<String>> = grouped.iter().map(|(heading, _)| heading.map(group_heading_text)).collect();
    let entries: Vec<(Option<&str>, ribbon::MenuEntry)> = grouped
        .iter()
        .zip(&labels)
        .zip(&headings)
        .map(|(((_, c), label), heading)| (heading.as_deref(), grid_entry(app, c, label, crate::toolbar_custom::pinned_on(app, tab, c.id).is_some())))
        .collect();
    let out = ribbon::grid(ui, &entries, loose, &menu_style(app));
    for cell in out.cells {
        if let Some((_, c)) = grouped.get(cell.index) {
            grid_cell_extras(app, ui, c, cell, paints);
        }
    }
    out.action.and_then(|(index, action)| grouped.get(index).map(|(_, c)| (c.id, action)))
}

/// Command `c` as a grid icon.
fn grid_entry<'a>(app: &SolveApp, c: &'static solidcraft_engine::CommandSpec, label: &'a str, pinned: bool) -> ribbon::MenuEntry<'a> {
    ribbon::MenuEntry {
        id: c.id,
        label,
        icon: c.icon,
        shortcut: "",
        enabled: c.info(&app.session).enabled,
        checked: None,
        pinned,
        tip: None,
        app_paints: app.preferences.persistent_ribbon_pressed && persistent_available(app, c.id),
    }
}

/// SolidCraft's part of a grid icon: it drags onto the toolbar, and its tooltip or its deferred
/// persistent paint.
fn grid_cell_extras(
    app: &SolveApp,
    ui: &egui::Ui,
    c: &'static solidcraft_engine::CommandSpec,
    cell: ribbon::GridCell,
    paints: &mut Vec<PersistentRibbonPaint>,
) {
    let info = c.info(&app.session);
    if let Some(deferred) = cell.deferred {
        // Reuse the native ribbon recipe; only responses/paint slots survive until dispatch.
        paints.push(PersistentRibbonPaint {
            command: c,
            deferred,
            label: None,
            enabled: ui.is_enabled() && info.enabled,
            disabled: info.disabled_reason,
        });
    } else {
        let _ = ribbon_tooltip(app, cell.response, c, info.disabled_reason.as_deref());
    }
}

use ribbon::ROW_H;

/// Registry parameter syntax is not a user-facing description. Reuse existing catalog prose
/// where present; other commands intentionally show only their name and registered shortcut.
fn ribbon_tooltip(app: &SolveApp, response: egui::Response, command: &solidcraft_engine::CommandSpec, disabled: Option<&str>) -> egui::Response {
    if let Some(hint) = persistent_hint(app, command, disabled.is_none() && response.enabled() && command.info(&app.session).enabled) {
        return response.backing_on_hover_text(hint);
    }
    response.backing_on_hover_ui(|ui| {
        let t = Tokens::get();
        let name = app.tr_label(crate::render_workspace::interactive_command_info(app, command).label);
        let name_width = ui
            .painter()
            .layout_no_wrap(name.to_string(), crate::ui_kit::type_face::semibold(solidcraft_theme::Tokens::get().font_heading), t.text)
            .size()
            .x;
        let key_width = crate::keymap::display_for(app, command.id)
            .map(|key| {
                ui.painter().layout_no_wrap(key.to_string(), FontId::monospace(solidcraft_theme::Tokens::get().font_body), t.text_dim).size().x + 24.0
            })
            .unwrap_or(0.0);
        ui.set_width(280.0_f32.max(name_width + key_width + 12.0));
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(name_width, 20.0), Sense::hover());
            ribbon::text(ui, rect.center(), name, t.font_heading, t.text, true);
            if let Some(key) = crate::keymap::display_for(app, command.id) {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    crate::ui_kit::Container::show_backing(
                        ui,
                        egui::Frame::NONE
                            .fill(t.panel_header)
                            .stroke(t.tertiary_border(t.border))
                            .corner_radius(t.radius_sm.saturating_sub(1))
                            .inner_margin(egui::Margin::symmetric(5, 1)),
                        |ui| {
                            ui.label(egui::RichText::new(key).monospace().color(t.text_dim));
                        },
                    );
                });
            }
        });
        let description = match command.id {
            "render.scene" => Some(app.tr("render.preview_help")),
            "render.start" => Some(app.tr("render.image_help")),
            // Sending again replaces the file the slicer has open.
            "file.send_to_slicer" => Some(app.tr("ui.slicer_resend_hint")),
            "render.preset.resolve" | "render.preset.apply" => Some(app.tr("render.ribbon_studios_help")),
            "render.library.appearance.apply" | "render.library.surface.bind" => Some(app.tr("render.ribbon_materials_help")),
            "render.library.studio.prepare" | "render.library.studio.apply" => Some(app.tr("render.ribbon_lights_help")),
            "render.library.environment.apply" | "render.stage.backdrop" => Some(app.tr("render.ribbon_environment_help")),
            "render.preflight" => Some(app.tr("render.ribbon_check_help")),
            "render.export" => Some(app.tr("render.output_format_status")),
            _ => None,
        };
        if let Some(description) = description {
            ui.add(egui::Label::new(description).wrap());
        }
        // The tool's own help, when it has one (#2758).
        if let Some(help) = crate::command_help::help(command.id) {
            ui.add(egui::Label::new(egui::RichText::new(help).color(t.text_dim)).wrap());
        }
        if let Some(reason) = disabled {
            ui.add(egui::Label::new(egui::RichText::new(app.tr_label(reason)).color(t.text_dim)).wrap());
        }
    })
}

/// A revealed command's tooltip, open without the pointer on it (#7287).
fn reveal_tooltip(app: &SolveApp, ui: &egui::Ui, rect: Rect, command: &solidcraft_engine::CommandSpec, align: egui::RectAlign) {
    let mut tip = egui::Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), egui::Id::unique(("ribbon_reveal", command.id)), rect);
    tip.popup = tip.popup.align(align);
    // The same text as a drop-down row's own tooltip: name and shortcut, its help, and why it is
    // disabled when it is.
    let info = crate::render_workspace::interactive_command_info(app, command);
    let label = app.tr_label(info.label);
    let shortcut = crate::keymap::display_for(app, command.id).map(|k| format!("  ({k})")).unwrap_or_default();
    let help = crate::command_help::help(command.id);
    if let Some(shown) = crate::ui_kit::Container::show_tooltip(egui::Id::unique(("ribbon_reveal", command.id)), tip, |ui| {
        ui.set_max_width(300.0);
        ui.label(egui::RichText::new(format!("{label}{shortcut}")).strong());
        if let Some(help) = help {
            ui.add_space(2.0);
            ui.label(egui::RichText::new(help).weak());
        }
        if let Some(reason) = info.disabled_reason.as_deref() {
            ui.label(egui::RichText::new(reason).weak());
        }
    }) {
        crate::scenario::publish_rect("ribbon:reveal_tooltip", shown.response.rect);
    }
}

/// A group heading's text: its name in the interface language, as written.
fn group_heading_text(label: &str) -> String {
    if label.starts_with("ui.") { solidcraft_ui_text::tr(label) } else { solidcraft_ui_text::tr_label(label) }.to_string()
}

/// Take up what was done in a grid drop-down, as for a list row: start the command (and close
/// the drop-down), open its menu, or pin or unpin it.
fn grid_action(
    app: &mut SolveApp,
    ui: &egui::Ui,
    action: Option<(&'static str, ribbon::MenuAction)>,
    menu: &mut Option<(egui::Pos2, &'static str)>,
    toggle: &mut Option<&'static str>,
) {
    match action {
        Some((id, ribbon::MenuAction::Menu(at))) => *menu = Some((at, id)),
        Some((id, ribbon::MenuAction::TogglePin)) => *toggle = Some(id),
        Some((id, ribbon::MenuAction::Run(rect))) => {
            app.start_at(id, Some(rect));
            ui.ctx().data_mut(|d| d.remove::<egui::Id>(egui::Id::unique("sc_panel_open")));
        }
        None => {}
    }
}

/// Sketch › Create in the grid: each family of variants under its own heading (Line, Rectangle,
/// Circle…), then the other tools under "More". The list keeps its family sub-menus.
fn sketch_create_grouped(cmds: &[&'static solidcraft_engine::CommandSpec]) -> Vec<(Option<&'static str>, &'static solidcraft_engine::CommandSpec)> {
    let mut out = Vec::new();
    for f in crate::sketch_families::FAMILIES {
        let mut heading = Some(f.label);
        for id in f.commands {
            if let Some(c) = cmds.iter().find(|c| c.id == *id) {
                out.push((heading.take(), *c));
            }
        }
    }
    let mut heading = Some("More");
    for c in cmds.iter().filter(|c| crate::sketch_families::family(c.id).is_none()) {
        out.push((heading.take(), *c));
    }
    out
}

/// A drop-down's commands without those the ribbon shows as buttons (`ribbon_menus_skip_pinned`),
/// unless that leaves none.
fn skip_on_ribbon(cmds: Vec<&'static solidcraft_engine::CommandSpec>, on_ribbon: &[&str]) -> Vec<&'static solidcraft_engine::CommandSpec> {
    let rest: Vec<_> = cmds.iter().copied().filter(|c| !on_ribbon.contains(&c.id)).collect();
    if rest.is_empty() { cmds } else { rest }
}

/// Creation menus keep related variants together, including collapsed and overflow panels.
/// The plain rows are `width` wide, the width the menu gives its family rows.
fn sketch_create_menu(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    cmds: &[&'static solidcraft_engine::CommandSpec],
    width: f32,
    menu: &mut Option<(egui::Pos2, &'static str)>,
    toggle: &mut Option<&'static str>,
) {
    let room = (ui.ctx().content_rect().bottom() - ui.cursor().top() - 64.0).max(ROW_H * 3.0);
    egui::ScrollArea::vertical().id_salt("sketch_create_rows").max_height(room).show(ui, |ui| {
        for f in crate::sketch_families::FAMILIES {
            let variants: Vec<_> = f.commands.iter().filter_map(|id| cmds.iter().find(|c| c.id == *id).copied()).collect();
            if let Some(first) = variants.first() {
                // The row translates its own label.
                let mut item = crate::ui_kit::MenuItem::new(f.label, f.label, first.icon);
                item.submenu = true;
                let response = crate::ui_kit::Menu::item_button(ui, &item, |ui| {
                    dropdown_keys(app, ui, &variants);
                    let variants_w = dropdown_width(app, ui, &variants);
                    sketch_menu_rows(app, ui, &variants, variants_w, menu, toggle);
                });
                crate::scenario::publish_handle(&format!("create_family:{}", f.label), response.response.rect.center());
            }
        }
        ui.separator();
        let rest: Vec<_> = cmds.iter().copied().filter(|c| crate::sketch_families::family(c.id).is_none()).collect();
        dropdown_keys(app, ui, &rest);
        sketch_menu_rows(app, ui, &rest, width, menu, toggle);
    });
}

fn sketch_menu_rows(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    cmds: &[&'static solidcraft_engine::CommandSpec],
    width: f32,
    menu: &mut Option<(egui::Pos2, &'static str)>,
    toggle: &mut Option<&'static str>,
) {
    for c in cmds {
        let pinned = crate::toolbar_custom::pinned_on(app, "SKETCH", c.id).is_some();
        // A hovered search result in a sketch creation menu is marked steadily (#7287).
        let revealed = app.sbox.reveal.as_ref().is_some_and(|r| r.preview && r.id == c.id);
        match dropdown_row(app, ui, c, width, pinned, revealed.then_some(1.0)) {
            None => {}
            Some(ribbon::MenuAction::Menu(at)) => *menu = Some((at, c.id)),
            Some(ribbon::MenuAction::TogglePin) => *toggle = Some(c.id),
            Some(ribbon::MenuAction::Run(rect)) => {
                app.start_at(c.id, Some(rect));
                ui.ctx().data_mut(|d| d.remove::<egui::Id>(egui::Id::unique("sc_panel_open")));
                ui.close();
            }
        }
    }
}

/// The SELECT panel: what a click in the view picks.
pub(crate) fn selection_filter(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui.label(egui::RichText::new(solidcraft_ui_text::tr_label("Selection filter")).color(t.text_dim).size(t.font_small));
    for (mode, label) in [("body", app.tr_label("Select bodies (a face picks its body)")), ("component", app.tr("ui.select_components_by_face"))] {
        let mut on = app.ui.pick_mode() == mode;
        if ui_kit::checkbox(ui, label, &mut on).changed() {
            // The Faces checkbox below is independent. The standalone command enables faces.
            let filters = std::mem::take(&mut app.ui.pick_off);
            let _ = app.run("select.filter", json!({"mode": if on { mode } else { "face" }}));
            app.ui.pick_off = filters;
        }
    }
    for (k, label) in [
        ("faces", "Faces"),
        ("edges", "Edges"),
        ("vertices", "Vertices"),
        ("sketch", "Sketch geometry and profiles"),
        ("sketch_curves", "Sketch curves"),
        ("sketch_points", "Sketch points"),
        ("dimensions", "Sketch dimensions"),
        ("constraints", "Sketch constraints"),
    ] {
        let mut on = !app.ui.pick_off.iter().any(|x| x == k);
        if crate::ui_kit::checkbox(ui, solidcraft_ui_text::tr_label(label), &mut on).changed() {
            app.ui.pick_off.retain(|x| x != k);
            if !on {
                app.ui.pick_off.push(k.to_string());
            }
        }
    }
    // The selection shape (#4376): a drag draws a freehand lasso instead of the rectangle.
    let label = app.tr("ui.lasso_select").to_string();
    ui_kit::checkbox(ui, &label, &mut app.ui.lasso_select);
}

/// A click on the lit button of the sketch tool that is running ends the tool, as a toggle does,
/// so clicks in the view select again (#10151). With Preferences › Sketch "Clicking the running
/// tool's button ends it" off, the click starts the tool again, as it did before. Returns whether
/// the tool was ended. A persistent analysis button already turns its analysis off its own way.
fn end_running_tool(app: &mut SolveApp, id: &str) -> bool {
    if !app.preferences.ribbon_click_ends_tool || persistent_available(app, id) || !app.tool.as_ref().is_some_and(|t| t.cmd == id) {
        return false;
    }
    crate::dim_view::cancel();
    app.tool = None;
    true
}

/// Two Esc presses within this many seconds finish the open sketch.
pub const ESCAPE_TWICE_S: f64 = 0.4;

/// Whether `cancel_unfinished` has something to cancel.
pub(crate) fn unfinished(app: &SolveApp) -> bool {
    app.tool.is_some()
        || crate::select_other::is_open(app)
        || app.menu.joint_move_warning.is_some()
        || app.viewport.joint_handles.is_some()
        || app.viewport.text_drag.is_some()
        || crate::occurrence_drag::active(app)
}

/// Cancels what one Esc cancels before it reaches the dialog, the selection or the sketch:
/// a drag, the Select Other list, or the active tool's points and then the tool. The view's
/// corner Cancel (#6709) takes this step first too. Returns whether anything was cancelled.
pub(crate) fn cancel_unfinished(app: &mut SolveApp) -> bool {
    if crate::joint_handles::cancel(app) || crate::sketch_text_move::cancel_drag(app) || crate::occurrence_drag::cancel(app) {
        return true;
    }
    if crate::select_other::is_open(app) {
        crate::select_other::close(app);
    } else if app.tool.is_some() {
        crate::tools::finish(app);
    } else {
        return false;
    }
    true
}

/// Global keyboard shortcuts (when no text field has focus).
pub fn shortcuts(app: &mut SolveApp, ctx: &egui::Context) {
    crate::viewport_status::reconcile(app, ctx);
    crate::browser::update_folder_pick(app, ctx);
    let chat_owner = crate::agent_chat::owns_keyboard_input(app, ctx);
    if design_shown(app) && !chat_owner {
        crate::navigation::step_key_hold(app, ctx);
    } else {
        app.viewport.key_hold = None;
    }
    if app.file_load_holds() {
        return;
    }
    if crate::viewport_status::enabled(app)
        && (app.esc_handled || app.escape_owner.is_some())
        && ctx.input(|input| input.key_pressed(egui::Key::Escape))
    {
        return;
    }
    // File is a custom Area, so it must claim Escape before the underlying editors and
    // sketch shortcuts. Clear raw events too: egui may replay them on a layout pass.
    let file_menu_id = egui::Id::unique("sc_file_menu");
    if !app.native_file_menu
        && ctx.data(|d| d.get_temp::<bool>(file_menu_id).unwrap_or(false))
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        ctx.data_mut(|d| d.insert_temp(file_menu_id, false));
        egui::Popup::close_all(ctx);
        ctx.input_mut(|i| {
            i.raw.events.retain(|event| !matches!(event, egui::Event::Key { key: egui::Key::Escape, modifiers, .. } if modifiers.matches_logically(egui::Modifiers::NONE)));
        });
        app.esc_handled = true;
        app.last_escape = None;
        return;
    }
    // The Keyboard Shortcuts window is waiting for a key: it is not a shortcut.
    if crate::keymap::capturing(app) {
        // Parameters is drawn before Preferences; do not let its footer see this Escape.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            crate::keymap::stop_capture(app);
            app.esc_handled = true;
        }
        return;
    }
    // These owners render after global shortcuts. Leave their Escape available to the
    // normal window/palette handler, but retain ownership if egui repeats the layout.
    if ctx.input(|input| input.key_pressed(egui::Key::Escape))
        && (app.prefs_window.shell.open || app.keymap.open || app.ui.palette_open || ctx.memory(|memory| memory.top_modal_layer().is_some()))
    {
        app.escape_owner = Some(crate::EscapeOwner::Overlay);
        if crate::viewport_status::enabled(app) {
            app.last_escape = None;
            return;
        }
    }
    if crate::params_dialog::cancel_on_escape(app, ctx) {
        return;
    }
    // A dimension value that is taking the keyboard: letters belong to it (a parameter name
    // starting with L or D), even on the frame before its field holds the focus.
    let popup_owner = egui::Popup::is_any_open(ctx)
        || crate::dialogs_save_mesh::popup_open(ctx)
        || crate::hole_presets::name_prompt_open(ctx, app.dialog.as_ref())
        || crate::personal_sheet_rules::name_prompt_open(ctx, app.dialog.as_ref());
    let inline_editor = crate::dim_view::editing();
    if !popup_owner
        && app.viewport_status.open_key.is_some()
        && crate::ui_kit::details_field_owned_escape(ctx)
        && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
        app.esc_handled = true;
        app.escape_owner = Some(crate::EscapeOwner::Overlay);
        return;
    }
    if popup_owner
        || inline_editor
        || chat_owner
        || (crate::viewport_status::enabled(app) && app.ui.palette_open)
        || (ctx.egui_wants_keyboard_input() && !(crate::viewport_status::enabled(app) && app.viewport_status.owns_focus(ctx)))
    {
        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            let owner = if app.dialog.is_some() && !popup_owner && !inline_editor { crate::EscapeOwner::Dialog } else { crate::EscapeOwner::Overlay };
            // Reserve first refusal without claiming that cancellation already occurred.
            // A dialog's own late handler must still be able to cancel its focused field.
            app.escape_owner.get_or_insert(owner);
            if crate::viewport_status::enabled(app) {
                app.last_escape = None;
            }
        }
        return;
    }
    // A quick chord can press and release all modifiers in one frame. Dispatch each key
    // with its own modifiers, not the final frame state (an unbound Ctrl+Shift+A is not A).
    let keys: Vec<_> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(
                |e| if let egui::Event::Key { key, pressed: true, repeat, modifiers, .. } = e { Some((*key, *repeat, *modifiers)) } else { None },
            )
            .collect()
    });
    // A theme pasted on the Interface page is a theme, not copied components (#8226).
    crate::prefs::take_theme_paste(app, ctx);
    // Home hides the design: only New, Open and Preferences work there.
    let shown = design_shown(app);
    if shown {
        crate::edit_clipboard::handle(app, ctx);
    } else {
        crate::edit_clipboard::flush(app, ctx);
    }
    for (k, repeat, mods) in keys {
        use egui::Key;
        // Only Esc pressed twice in a row finishes a sketch: any other key in between resets it.
        if k != Key::Escape {
            app.last_escape = None;
        }
        if shown && (crate::keymap::drag_zoom_key(app, k, mods) || crate::navigation::view_key(app, ctx, k, mods, repeat)) {
            continue;
        }
        if mods.command {
            match k {
                // An editing embed's host has its own New, Open, Save, Close and settings.
                Key::N | Key::O | Key::S | Key::W | Key::Comma if app.embed_editor => {}
                Key::N => crate::documents::new_design(app),
                Key::Comma => crate::prefs::open_at(app, 0),
                Key::O => {
                    app.pick_open(|app, path| {
                        if let Some(path) = path {
                            app.open_path_async(&path);
                        }
                    });
                }
                _ if !shown => {}
                // Close the design shown (asking first when it is unsaved); closing the last one
                // shows the start page (#5366).
                Key::W if !mods.shift && !mods.alt => crate::documents::close(app, app.docs.active, false),
                Key::Z => {
                    if !crate::tools::undo_spline_point(app) {
                        drop(app.run("edit.undo", json!({})));
                    }
                }
                Key::Y => drop(app.run("edit.redo", json!({}))),
                // Copy, Cut and Paste, and Paste New with Shift (`edit_clipboard::handle`).
                Key::C | Key::X if !mods.shift && !mods.alt => {}
                Key::V if !mods.alt => {}
                // Ctrl+Shift+S is Save As; Ctrl+S keeps saving over the current file (#6558).
                Key::S if mods.shift && !mods.alt => save_as(app),
                Key::S => save(app),
                _ => crate::keymap::press(app, &crate::keymap::key_name(k, mods), repeat),
            }
            continue;
        }
        if !shown {
            continue;
        }
        match k {
            Key::Escape => {
                app.esc_handled = true;
                if crate::render_workspace::result_active(app) {
                    crate::keymap::press(app, "Escape", repeat);
                    continue;
                }
                // End navigation before the underlying tool or dialog; sketch double-Escape
                // handling starts with the next press.
                if app.viewport.nav.take().is_some() {
                    app.last_escape = None;
                    continue;
                }
                if crate::render_workspace::assign::cancel(app) {
                    app.last_escape = None;
                    continue;
                }
                // Two quick presses in a sketch finish it (keeping it, like Finish Sketch);
                // the first one has already cancelled whatever was active. A preference turns
                // this off, so Esc never leaves the sketch (#12069).
                let now = ctx.input(|i| i.time);
                let again = app.preferences.escape_twice_finishes_sketch && app.last_escape.is_some_and(|t| now - t <= ESCAPE_TWICE_S);
                app.last_escape = Some(now);
                // Not while a tool is still out (its first Esc only ended a chain): this press
                // puts it down, and the next quick one finishes.
                if again
                    && !(crate::viewport_status::enabled(app) && (app.viewport_status.open_key.is_some() || crate::viewport_status::has_effect(app)))
                    && app.tool.is_none()
                    && app.dialog.is_none()
                    && let Some(sketch) = app.session.active_sketch
                {
                    app.last_escape = None;
                    // A sketch left blank is taken away instead, in one undo step, so no empty
                    // sketch stays behind.
                    let blank = app.session.doc.sketch(sketch).is_ok_and(crate::sketch_palette::is_blank);
                    let depth = app.session.undo.len();
                    app.finish_sketch();
                    if blank && app.session.active_sketch.is_none() && app.run("timeline.delete", json!({ "feature": sketch })).is_ok() {
                        app.session.undo.truncate(depth + 1);
                    }
                    continue;
                }
                if cancel_unfinished(app) {
                    continue;
                }
                if app.dialog.is_some() || app.pending_feature_edit.is_some() {
                    crate::dialogs::cancel(app);
                } else if crate::viewport_status::enabled(app) && crate::viewport_status::escape(app, repeat) {
                    app.last_escape = None;
                } else if !app.session.selection.is_empty() || app.session.active_sketch.is_none() {
                    crate::edit_clipboard::drop_cut(app);
                    app.tree.picked_joints.clear();
                    let _ = app.run("select.clear", json!({}));
                }
                // A single Esc with nothing left to cancel keeps the sketch open (#1804); a quick
                // second press finishes it (above).
            }
            Key::F2 => {
                if !crate::context_menu::press_shortcut(app, "F2") {
                    let at = ctx.input(|i| i.pointer.latest_pos()).unwrap_or(ctx.content_rect().center());
                    crate::context_menu::rename_selection(app, at);
                }
            }
            Key::F6 => app.animate_view("fit"),
            Key::Backspace if crate::tools::undo_spline_point(app) => {}
            Key::Delete | Key::Backspace => {
                if !crate::timeline_left::delete_selected_folders(app) {
                    crate::viewport::delete_selection(app);
                }
            }
            _ => crate::keymap::press(app, &crate::keymap::key_name(k, mods), repeat),
        }
    }
}

/// The Report a bug button for this header layout ([`app_bar::report_rect`]).
#[cfg(test)]
fn report_rect(bar: Rect, captions: f32, tabs_start: f32, label_width: f32, version_width: f32, layout: HeaderLayout) -> (Rect, bool) {
    let version = (layout.version == VersionPlace::Header).then_some(version_width);
    app_bar::report_rect(bar, captions, tabs_start, label_width, version, layout.report == ReportButton::Icon)
}

#[cfg(test)]
use app_bar::{CLUSTER_BUTTONS, REPORT_TEXT_X, RIGHT_GROUP, SEARCH_GAP as HEADER_SEARCH_GAP, help_x, version_room};

/// A grid drop-down's commands as the grid lays them out: their ids under their headings.
#[cfg(test)]
fn grid_ids(grouped: &[(Option<&'static str>, &'static solidcraft_engine::CommandSpec)]) -> Vec<(Option<&'static str>, &'static str)> {
    grouped.iter().map(|(heading, c)| (*heading, c.id)).collect()
}

/// One icon of a grid drop-down at `rect` ([`ribbon::grid_cell`]).
#[cfg(test)]
fn dropdown_cell(
    app: &SolveApp,
    ui: &mut egui::Ui,
    c: &'static solidcraft_engine::CommandSpec,
    rect: Rect,
    pinned: bool,
    paints: &mut Vec<PersistentRibbonPaint>,
) -> Option<ribbon::MenuAction> {
    let label = app.tr_label(c.label).to_string();
    let (action, cell) = ribbon::grid_cell(ui, &grid_entry(app, c, &label, pinned), rect, &menu_style(app));
    grid_cell_extras(app, ui, c, cell, paints);
    action
}

#[cfg(test)]
mod header_report_tests {
    use super::*;

    #[test]
    fn escape_closes_file_menu_on_home_and_over_a_sketch_without_cancelling_it() {
        for home in [true, false] {
            let mut h = crate::scenario::Harness::new();
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            h.app.run("sketch.line", json!({"points":[[0,0],[10,0]]})).unwrap();
            h.app.start("sketch.line");
            h.app.home.open = home;
            h.frames(3);
            let sketch = h.app.session.active_sketch;
            let before = h.app.session.doc.to_json();
            let menu_open = |h: &crate::scenario::Harness| h.ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("sc_file_menu")).unwrap_or(false));
            h.step(&json!({"click":{"handle":"appbar:file"}})).unwrap();
            assert!(menu_open(&h));
            h.app.last_escape = Some(h.ctx.input(|i| i.time));
            h.step(&json!({"key":"Escape"})).unwrap();
            assert!(!menu_open(&h), "Escape closes File on Home and in the design");
            assert_eq!(h.app.home.open, home);
            assert_eq!(h.app.session.active_sketch, sketch);
            assert!(h.app.tool.is_some(), "closing File must not cancel the sketch tool");
            assert_eq!(h.app.session.doc.to_json(), before);
            assert!(h.app.last_escape.is_none(), "closing File must not arm Finish Sketch");
        }
    }

    #[test]
    fn escape_closes_file_menu_before_parameters_and_preserves_selection() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("parameters.change", json!({"name":"width","expression":"10 mm"})).unwrap();
        h.app.session.selection = vec![solidcraft_engine::Sel::Plane { name: "XY".into() }];
        h.step(&json!({"start":"parameters.change"})).unwrap();
        let before = (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.selection.clone());
        h.step(&json!({"click":{"handle":"appbar:file"}})).unwrap();
        assert!(h.ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("sc_file_menu")).unwrap_or(false)));
        h.step(&json!({"key":"Escape"})).unwrap();
        assert!(!h.ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("sc_file_menu")).unwrap_or(false)));
        assert!(h.app.params.is_some(), "the first Escape belongs to File");
        assert_eq!(before, (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.selection.clone()));
        h.step(&json!({"key":"Escape"})).unwrap();
        assert!(h.app.params.is_none(), "the next Escape still closes Parameters");
        assert_eq!(before, (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.selection.clone()));
    }

    #[test]
    fn native_file_menu_preserves_application_bar_controls() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length":2,"width":3,"height":4})).unwrap();
        for native in [false, true] {
            h.app.native_file_menu = native;
            h.app.integrated_titlebar = native;
            h.frames(3);
            for name in [
                "appbar:file",
                "appbar:save",
                "appbar:undo",
                "appbar:redo",
                "appbar:help",
                "titlebar:preferences",
                "titlebar:assistant",
                "titlebar:report",
            ] {
                assert!(crate::scenario::handle_at(name).is_some(), "native={native}: {name}");
            }
            let tabs = crate::scenario::rect_of("tab:design:0").unwrap();
            let assistant = crate::scenario::rect_of("titlebar:assistant").unwrap();
            assert!(tabs.right() < assistant.left(), "tabs leave room for the right controls");
            let depth = h.app.session.undo.len();
            h.step(&json!({"click":{"handle":"appbar:undo"}})).unwrap();
            assert_eq!(h.app.session.undo.len(), depth - 1);
            h.step(&json!({"click":{"handle":"appbar:redo"}})).unwrap();
            assert_eq!(h.app.session.undo.len(), depth);
            h.step(&json!({"click":{"handle":"appbar:file"}})).unwrap();
            assert!(h.widget("Save As JSON…").is_some(), "the in-window File menu still opens");
            h.step(&json!({"click":{"handle":"appbar:file"}})).unwrap();
            assert!(!h.ctx.data(|data| data.get_temp::<bool>(egui::Id::unique("sc_file_menu")).unwrap_or(false)));
            h.step(&json!({"click":{"handle":"appbar:help"}})).unwrap();
            assert!(h.app.help.menu);
            h.step(&json!({"key":"Escape"})).unwrap();
            h.step(&json!({"click":{"handle":"titlebar:assistant"}})).unwrap();
            assert!(h.app.agent_chat.open);
            h.step(&json!({"click":{"handle":"titlebar:assistant"}})).unwrap();
            assert!(!h.app.agent_chat.open);
        }
    }

    #[test]
    fn native_menu_actions_share_undo_and_redo_history() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        assert!(!FileAction::Undo.enabled(&h.app));
        assert!(!FileAction::Redo.enabled(&h.app));
        h.app.run("solid.box", json!({"length":2,"width":3,"height":4})).unwrap();
        let depth = h.app.session.undo.len();
        assert!(FileAction::Undo.enabled(&h.app));
        FileAction::Undo.run(&mut h.app);
        assert_eq!(h.app.session.undo.len(), depth - 1);
        assert!(FileAction::Redo.enabled(&h.app));
        FileAction::Redo.run(&mut h.app);
        assert_eq!(h.app.session.undo.len(), depth);
        assert!(!FileAction::Redo.enabled(&h.app));
    }

    #[test]
    fn native_menu_actions_share_settings_and_unsaved_quit() {
        let mut h = crate::scenario::Harness::new();
        FileAction::Settings.run(&mut h.app);
        assert!(h.app.prefs_window.shell.open);
        h.app.run("solid.box", json!({"length":2,"width":3,"height":4})).unwrap();
        FileAction::Quit.run(&mut h.app);
        assert!(!h.app.quit_requested);
        assert!(h.app.docs.quitting.is_some());
    }

    #[test]
    fn native_menu_actions_disable_save_on_home() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = true;
        assert!(!FileAction::Save.enabled(&h.app));
        assert!(!FileAction::SaveAs.enabled(&h.app));
        assert!(FileAction::New.enabled(&h.app));
        assert!(FileAction::Open.enabled(&h.app));
        assert!(FileAction::Settings.enabled(&h.app));
        assert!(!FileAction::Export("step").enabled(&h.app));
        assert!(!FileAction::ExportMesh("stl").enabled(&h.app));
        assert!(!FileAction::Command("document.metadata").enabled(&h.app));
        h.app.home.open = false;
        assert!(FileAction::Save.enabled(&h.app));
        assert!(FileAction::Export("step").enabled(&h.app));
    }

    /// File › Export STEP…, STL…, 3MF…, OBJ… and URDF… each run a command of their own, so each
    /// can be given a key in Keyboard Shortcuts: the key asks where to save the file as the menu
    /// item does (STL and 3MF open Save As Mesh).
    #[test]
    fn each_export_format_has_a_key_that_starts_it() {
        let mut h = crate::scenario::Harness::new();
        let offered = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(String, Vec<String>)>::new()));
        let (seen, dir) = (offered.clone(), h.dir.clone());
        h.app.services.pick_save = Some(Box::new(move |name, extensions| {
            seen.borrow_mut().push((name.to_string(), extensions.iter().map(|e| e.to_string()).collect()));
            Box::pin(std::future::ready(Some(dir.join(name).to_string_lossy().into_owned())))
        }));
        h.app.run("solid.box", json!({"length":2,"width":3,"height":4})).unwrap();
        h.app.session.doc_mut().name = "Bracket".into();
        for (id, key) in [("file.export_step", "Alt+Shift+F7"), ("file.export_stl", "Alt+Shift+F8")] {
            crate::keymap::bind(&mut h.app, id, key, false).unwrap();
            assert_eq!(crate::keymap::effective(&h.app, id).as_deref(), Some(key), "the File menu shows it");
        }
        crate::keymap::press(&mut h.app, "Alt+Shift+F7", false);
        h.frames(3);
        h.settle();
        assert_eq!(offered.borrow().last().cloned(), Some(("Bracket.step".to_string(), vec!["step".to_string()])));
        let step = std::fs::read(h.dir.join("Bracket.step")).unwrap();
        assert!(step.starts_with(b"ISO-10303-21;"), "a STEP file is written");

        crate::keymap::press(&mut h.app, "Alt+Shift+F8", false);
        assert_eq!(h.app.dialog.as_ref().and_then(|d| d.command()), Some("file.save_mesh"), "STL opens Save As Mesh");
        crate::dialogs::cancel(&mut h.app);
    }

    /// New designs are saved as `.sc`. A design opened from a `.solidcraft` file is saved back to
    /// that file by Save (no rename behind the user's back); Save As offers `.sc` and leaves it.
    #[test]
    fn save_offers_sc_and_keeps_an_opened_solidcraft_file() {
        let mut h = crate::scenario::Harness::new();
        let offered = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(String, Vec<String>)>::new()));
        let (seen, dir) = (offered.clone(), h.dir.clone());
        h.app.services.pick_save = Some(Box::new(move |name, extensions| {
            seen.borrow_mut().push((name.to_string(), extensions.iter().map(|e| e.to_string()).collect()));
            let path = dir.join(name).to_string_lossy().into_owned();
            Box::pin(std::future::ready(Some(path)))
        }));
        h.app.run("solid.box", json!({"length":2,"width":3,"height":4})).unwrap();
        h.app.session.doc_mut().name = "Bracket".into();
        FileAction::Save.run(&mut h.app);
        h.frames(3);
        h.settle();
        assert_eq!(offered.borrow().last().map(|(n, e)| (n.as_str(), e.clone())), Some(("Bracket.sc", vec!["sc".to_string(), "json".to_string()])));
        assert!(h.dir.join("Bracket.sc").is_file(), "a new design is saved as .sc");

        // A design from an earlier release.
        let old = h.dir.join("Old.solidcraft");
        std::fs::rename(h.dir.join("Bracket.sc"), &old).unwrap();
        h.app.run("doc.open", json!({"path": old.to_string_lossy()})).unwrap();
        h.app.run("solid.box", json!({"length":1,"width":1,"height":1})).unwrap();
        let asked = offered.borrow().len();
        FileAction::Save.run(&mut h.app);
        h.frames(3);
        h.settle();
        assert_eq!(offered.borrow().len(), asked, "Save writes the opened file without asking");
        assert_eq!(h.app.session.path.as_deref(), Some(old.to_string_lossy().as_ref()));
        assert!(!h.app.session.is_dirty());
        assert_eq!(solidcraft_engine::io::read_design(&std::fs::read(&old).unwrap()).unwrap().features.len(), 2);
        assert!(!h.dir.join("Old.sc").exists() && !h.dir.join("Bracket.sc").exists(), "nothing renamed");

        FileAction::SaveAs.run(&mut h.app);
        h.frames(3);
        h.settle();
        let name = offered.borrow().last().map(|(n, _)| n.clone()).unwrap();
        assert!(name.ends_with(".sc"), "Save As offers .sc: {name}");
        assert!(h.dir.join(&name).is_file() && old.is_file(), "Save As leaves the .solidcraft file as it was");
    }

    /// #8329: Save As (and Save a Copy) offer the name of the file the design came from, not the
    /// name stored in the design, which a copy of the file still carries; a design renamed since
    /// it was opened is offered its new name.
    #[test]
    fn save_as_offers_the_open_files_own_name() {
        let mut h = crate::scenario::Harness::new();
        let offered = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let (seen, dir) = (offered.clone(), h.dir.clone());
        h.app.services.pick_save = Some(Box::new(move |name, _| {
            seen.borrow_mut().push(name.to_string());
            Box::pin(std::future::ready(Some(dir.join(name).to_string_lossy().into_owned())))
        }));
        let ask = |h: &mut crate::scenario::Harness, how: fn(&mut SolveApp)| {
            how(&mut h.app);
            h.frames(3);
            h.settle();
            offered.borrow().last().cloned().unwrap_or_default()
        };
        h.app.run("solid.box", json!({"length":2,"width":3,"height":4})).unwrap();
        h.app.session.doc_mut().name = "Sample Plate".into();
        assert_eq!(ask(&mut h, save_as), "Sample Plate.sc", "a new design is offered its own name");
        // The same design under another file name, as copies of a file are (the report's
        // "Sample Plate-bug.solidcraft"): it is still named "Sample Plate" inside.
        let copy = h.dir.join("Sample Plate-bug.solidcraft");
        std::fs::copy(h.dir.join("Sample Plate.sc"), &copy).unwrap();
        h.app.run("doc.open", json!({"path": copy.to_string_lossy()})).unwrap();
        assert_eq!(h.app.session.doc.name, "Sample Plate");
        assert_eq!(ask(&mut h, save_as), "Sample Plate-bug.sc");
        // Saved under that name, the file is the one Save As offers next time.
        assert_eq!(h.app.session.path.as_deref(), Some(h.dir.join("Sample Plate-bug.sc").to_string_lossy().as_ref()));
        assert_eq!(ask(&mut h, save_copy), "Sample Plate-bug copy.sc");
        h.app.run("doc.open", json!({"path": copy.to_string_lossy()})).unwrap();
        // Renamed since it was opened: the new name.
        h.app.run("component.rename", json!({"component": 0, "name": "Plate Mk2"})).unwrap();
        assert_eq!(ask(&mut h, save_as), "Plate Mk2.sc");
    }

    #[test]
    fn native_menu_actions_are_disabled_behind_an_open_issue_report() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length":2,"width":3,"height":4})).unwrap();
        h.app.home.recent = vec!["elsewhere.solidcraft".into()];
        h.app.issue_report.draft = Some(crate::issue_report::draft_for(&h.app, None, None));
        let before = (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.docs.count(), h.app.home.recent.clone());
        for action in [
            FileAction::Undo,
            FileAction::New,
            FileAction::Open,
            FileAction::OpenRecent("elsewhere.solidcraft".into()),
            FileAction::ClearRecent,
            FileAction::Command("file.insert_step"),
            FileAction::Save,
            FileAction::SaveAs,
            FileAction::SaveJson,
            FileAction::ExportMesh("stl"),
            FileAction::Export("step"),
            FileAction::Export("json"),
            FileAction::Command("document.metadata"),
            FileAction::Settings,
            FileAction::Shortcuts,
            FileAction::AgentChat,
        ] {
            assert!(!action.enabled(&h.app), "{action:?} behind the report");
            action.run(&mut h.app);
        }
        assert_eq!(before, (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.docs.count(), h.app.home.recent.clone()));
        assert!(
            h.app.dialog.is_none() && h.app.file_picker.is_none() && !h.app.prefs_window.shell.open && !h.app.keymap.open && !h.app.agent_chat.open
        );
        // A focused report field still gets the menu's Undo and Redo.
        assert!(FileAction::Undo.menu_enabled(&h.app, true) && !FileAction::Undo.menu_enabled(&h.app, false));
        assert!(!FileAction::Save.menu_enabled(&h.app, true));
        // Quit waits as behind any modal window: it would switch to a design with unsaved changes
        // under the report.
        assert!(!FileAction::Quit.enabled(&h.app));
        FileAction::Quit.run(&mut h.app);
        assert!(h.app.docs.quitting.is_none() && h.app.docs.closing.is_none());
        // Help's windows wait too; Report an Issue… shows the report.
        for item in ["about", "shortcuts", "messages", "crashes", "connect", "updates"] {
            assert!(!crate::help::available(&h.app, item), "{item}");
            crate::help::run(&mut h.app, &h.ctx, item);
        }
        assert!(!h.app.help.about && !h.app.keymap.open && !h.app.messages.open && h.app.help.connect.is_none());
        assert!(!h.app.updater.open);
        assert!(crate::help::available(&h.app, "report") && crate::help::available(&h.app, "docs"));
        h.app.issue_report.draft = None;
        assert!(FileAction::Undo.enabled(&h.app) && FileAction::Settings.enabled(&h.app) && FileAction::Quit.enabled(&h.app));
        assert!(crate::help::available(&h.app, "shortcuts"));
    }

    #[test]
    fn native_menu_actions_share_json_save_and_recent_cleanup() {
        let mut h = crate::scenario::Harness::new();
        let path = h.dir.join("native-menu.json");
        let output = path.clone();
        h.app.services.pick_save = Some(Box::new(move |_, _| Box::pin(std::future::ready(Some(output.to_string_lossy().into_owned())))));
        h.app.run("solid.box", json!({"length":2,"width":3,"height":4})).unwrap();
        FileAction::SaveJson.run(&mut h.app);
        h.frames(3);
        h.settle();
        let bytes = std::fs::read(path).unwrap();
        assert!(serde_json::from_slice::<serde_json::Value>(&bytes).is_ok());
        assert!(!h.app.home.recent.is_empty());
        FileAction::ClearRecent.run(&mut h.app);
        assert!(h.app.home.recent.is_empty());
        assert!(h.app.home.opened_at.is_empty());
        assert!(!FileAction::ClearRecent.enabled(&h.app));
    }

    #[test]
    fn file_menu_json_save_and_export_use_commands_and_keep_distinct_paths() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length":2,"width":3,"height":4})).unwrap();
        let dir = h.dir.clone();
        h.app.services.pick_save =
            Some(Box::new(move |name, _extensions| Box::pin(std::future::ready(Some(dir.join(name).to_string_lossy().into_owned())))));
        h.frames(3);
        h.step(&json!({"click":{"handle":"appbar:folder"}})).unwrap();
        h.step(&json!({"widget":"Save As JSON…"})).unwrap();
        // Written on a worker: the window keeps drawing until it is done.
        h.settle();
        let saved = h.app.session.path.clone().unwrap();
        let bytes = std::fs::read(&saved).unwrap();
        assert!(!solidcraft_engine::io::is_design_container(&bytes));
        assert_eq!(solidcraft_engine::io::read_design(&bytes).unwrap(), *h.app.session.doc);
        h.app.session.doc_mut().name = "Exported copy".into();
        h.frames(3);
        h.step(&json!({"click":{"handle":"appbar:folder"}})).unwrap();
        h.step(&json!({"widget":"Export Design as JSON…"})).unwrap();
        assert_eq!(h.app.session.path.as_deref(), Some(saved.as_str()));
        assert!(h.dir.join("Exported copy.json").is_file());
        h.app.run("file.save", json!({})).unwrap();
        assert!(!solidcraft_engine::io::is_design_container(&std::fs::read(saved).unwrap()), "subsequent Save keeps a .json choice");
    }

    /// File › Export STL… and Export 3MF… both open the Save as mesh dialog with their format
    /// chosen (whatever was used last), and command search finds the export by format or purpose.
    #[test]
    fn file_menu_has_export_stl_and_3mf_that_open_the_mesh_dialog_with_that_format() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length":2,"width":3,"height":4})).unwrap();
        h.app.preferences.save_mesh_format = "3mf".into();
        for (item, format) in [("Export STL…", "stl"), ("Export 3MF…", "3mf")] {
            h.frames(3);
            h.step(&json!({"click":{"handle":"appbar:file"}})).unwrap();
            h.step(&json!({"widget": item})).unwrap();
            let Some(crate::dialogs::Kind::SaveMesh(k)) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("{item}: no Save as mesh dialog") };
            assert_eq!(k.format_name(), format, "{item}");
            crate::dialogs::cancel(&mut h.app);
        }
        for q in ["3mf", "stl", "export 3mf", "print"] {
            let found = crate::shortcut_box::search(&h.app, q);
            assert!(found.iter().any(|r| r.0 == "file.save_mesh"), "{q}: {:?}", found.iter().map(|r| r.0).collect::<Vec<_>>());
        }
    }

    /// The browser build's File > Download design hands the open design to the host as a
    /// `.solidcraft` container named after it; a host that saves real files shows no such item.
    #[test]
    fn file_menu_download_design_hands_the_container_to_the_host() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length":2,"width":3,"height":4})).unwrap();
        h.app.session.doc_mut().name = "Plate".into();
        let got = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(String, Vec<u8>)>::new()));
        let sink = got.clone();
        h.app.services.download_design = Some(Box::new(move |name, bytes| {
            sink.borrow_mut().push((name.to_string(), bytes.to_vec()));
            Ok(())
        }));
        h.frames(3);
        h.step(&json!({"click":{"handle":"appbar:folder"}})).unwrap();
        h.step(&json!({"widget":"Download design"})).unwrap();
        let got = got.borrow();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, "Plate.sc");
        assert!(solidcraft_engine::io::is_design_container(&got[0].1));
        assert!(h.app.session.path.is_none(), "a download is no save: the design keeps no file");
    }

    /// Ctrl+Shift+S opens Save As in the design workspace (it never saves over the current file),
    /// and the File menu shows that shortcut next to Save As (#6558).
    #[test]
    fn ctrl_shift_s_opens_save_as_in_the_workspace_and_the_file_menu_shows_the_shortcut() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length":2,"width":3,"height":4})).unwrap();
        let source = h.dir.join("source.solidcraft");
        h.app.run("file.save_as", json!({"path":source})).unwrap();
        let bytes = std::fs::read(&source).unwrap();
        h.app.session.doc_mut().name = "Copy".into();
        h.frames(3);
        h.step(&json!({"key":"S","cmd":true,"shift":true})).unwrap();
        h.frames(3);
        h.settle();
        assert_eq!(h.app.session.path.as_deref(), h.dir.join("Copy.sc").to_str(), "Ctrl+Shift+S must open Save As, not save over the current file");
        assert_eq!(std::fs::read(&source).unwrap(), bytes, "Save As keeps the file the design came from");
        // The File menu shows the shortcut of the command it runs.
        let spec = solidcraft_engine::find_command("file.save_as").expect("file.save_as is a command");
        assert_eq!(spec.shortcut, Some("Ctrl+Shift+S"), "the File menu shows the shortcut of Save As");
        h.frames(3);
        h.step(&json!({"click":{"handle":"appbar:file"}})).unwrap();
        assert!(h.widget("Save As…").is_some(), "the File menu has a Save As row");
    }

    fn captured_frame(h: &mut crate::scenario::Harness) {
        let token = h.app.shots.token;
        crate::issue_report::receive(&mut h.app, token, std::sync::Arc::new(egui::ColorImage::new([2, 2], vec![Color32::GREEN; 4])));
        h.frames(3);
    }

    #[test]
    fn panel_accents_share_the_tab_mapping_but_constraints_keep_their_role() {
        for dark in [false, true] {
            for vision in solidcraft_theme::ColorVision::ALL {
                let t = Tokens::themed(dark, vision);
                for tab in TABS.iter().copied().chain(["SKETCH", "FORM", "RENDER"]) {
                    assert_eq!(panel_accent(&t, tab, "CREATE"), crate::workspace::tab_accent(&t, tab).color);
                    assert_eq!(panel_accent(&t, tab, "CONSTRAIN"), t.glyph_text);
                }
            }
        }
    }

    #[test]
    fn header_report_reserves_controls_at_normal_and_double_scale_widths() {
        for physical_width in [900.0, 1400.0] {
            for scale in [1.0, 2.0] {
                let width = physical_width / scale;
                let bar = Rect::from_min_size(egui::Pos2::ZERO, vec2(width, APP_BAR_HEIGHT));
                for captions in [0.0, crate::window_chrome::WIDTH] {
                    // The English label, and a longer one (Russian is about 125 px).
                    for (label_width, layout) in [80.0, 125.0].into_iter().flat_map(|w| [(w, HeaderLayout::DESKTOP), (w, HeaderLayout::COMPACT)]) {
                        let (report, labeled) = report_rect(bar, captions, 354.0, label_width, 40.0, layout);
                        assert!(bar.contains_rect(report));
                        let prefs = Rect::from_center_size(pos2(report.left() - 52.0, APP_BAR_HEIGHT / 2.0), vec2(32.0, 32.0));
                        let help = Rect::from_center_size(pos2(help_x(report), APP_BAR_HEIGHT / 2.0), vec2(32.0, 32.0));
                        assert!(!report.intersects(prefs) && !report.intersects(help) && !help.intersects(prefs));
                        assert!(prefs.right() < help.left() && help.right() < report.left());
                        if layout.report == ReportButton::Icon {
                            assert!(!labeled && report.size() == vec2(32.0, 32.0));
                            continue;
                        }
                        let full = REPORT_TEXT_X + label_width + 11.0;
                        assert_eq!(
                            labeled,
                            width - captions - version_room(bar, captions, 354.0, 40.0).unwrap_or(8.0) - CLUSTER_BUTTONS - 354.0 >= 216.0 + full
                        );
                        if labeled {
                            assert!(REPORT_TEXT_X + label_width < report.width(), "{label_width}: the text overruns its button");
                        }
                    }
                }
            }
        }
    }

    /// The 34 px bar groups brand/home, File/Save/Undo/Redo, documents, then the right cluster.
    #[test]
    fn title_bar_keeps_tabs_left_and_help_right() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.custom_titlebar = true;
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
        h.call("ui.resize", json!({"width":1400,"height":720}));
        h.frames(3);
        for gone in ["appbar:sun", "appbar:moon", "toolbar:ai"] {
            assert!(crate::scenario::handle_at(gone).is_none(), "{gone} is no longer in the title bar");
        }
        let at = |name: &str| crate::scenario::handle_at(name).unwrap_or_else(|| panic!("{name}")).x;
        let [report, _] = h.widget("Report a bug").unwrap();
        let file = h.widget("File").expect("the File menu has a label");
        assert_eq!(file[0] as f32, at("appbar:file"));
        assert_eq!(APP_BAR_HEIGHT, 34.0);
        let rect = |name: &str| crate::scenario::rect_of(name).unwrap();
        let first = rect("titlebar:separator:brand");
        let second = rect("titlebar:separator:documents");
        assert_eq!(first.size(), vec2(1.0, 20.0));
        assert_eq!(second.size(), vec2(1.0, 20.0));
        assert_eq!(first.center().y, 17.0);
        assert_eq!(second.center().y, 17.0);
        assert!(rect("titlebar:home").right() < first.left());
        assert!(first.right() < rect("appbar:file").left());
        assert!(at("appbar:file") < at("titlebar:save") && at("titlebar:save") < at("appbar:undo") && at("appbar:undo") < at("appbar:redo"));
        assert!(rect("appbar:redo").right() < second.left());
        assert!(second.right() < rect("tab:design:0").left());
        assert!(at("tab:design:0") < at("titlebar:assistant"));
        assert!(at("titlebar:assistant") < at("titlebar:preferences"));
        for name in ["titlebar:assistant", "titlebar:preferences", "appbar:help"] {
            assert_eq!(rect(name).size(), vec2(32.0, 32.0));
            assert!(rect(name).top() >= 0.0 && rect(name).bottom() <= APP_BAR_HEIGHT);
        }
        assert!(at("titlebar:preferences") < at("appbar:help") && (at("appbar:help") as f64) < report);
        assert!(at("appbar:home") < at("appbar:file"));
        assert!(h.widget("Start page").is_some(), "the house button has an accessible name");
        h.step(&json!({"click":{"handle":"appbar:help"}})).unwrap();
        assert!(h.app.help.menu, "? opens the Help menu");
        h.step(&json!({"key":"Escape"})).unwrap();
        h.step(&json!({"click":{"handle":"appbar:home"}})).unwrap();
        assert!(h.app.home.open, "the house button shows the start page");
        let dark = h.app.ui.dark;
        h.call("ui.prefs", json!({"theme": if dark { "light" } else { "dark" }}));
        h.frames(2);
        assert_ne!(h.app.ui.dark, dark, "the theme is a preference");
    }

    #[test]
    fn header_report_button_widens_for_a_longer_translation() {
        let width_in = |language: &str| {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.app.custom_titlebar = true;
            h.app.preferences.language = language.into();
            h.call("ui.resize", json!({"width":1400.0,"height":720}));
            h.frames(3);
            let center = crate::scenario::handle_at("titlebar:report").expect("the report button is drawn");
            let t = Tokens::get();
            let version = format!("v{}", crate::update::current_version());
            let version = h.ctx.fonts_mut(|f| f.layout_no_wrap(version, FontId::proportional(t.font_small), t.app_bar_text).size().x);
            // The button's right edge is fixed; its width is twice the distance to its centre.
            let bar = Rect::from_min_size(egui::Pos2::ZERO, vec2(1400.0, APP_BAR_HEIGHT));
            let room = version_room(bar, crate::window_chrome::WIDTH, 354.0, version).unwrap_or(8.0);
            2.0 * (1400.0 - crate::window_chrome::WIDTH - room - center.x)
        };
        let (en, ru) = (width_in("en"), width_in("ru"));
        assert!((en - 114.34375).abs() < 0.05, "Plex Sans report button width: {en}");
        // "Сообщить об ошибке" needs more than the English button's 114 px.
        assert!(ru > en + 10.0, "{ru}");
    }

    #[test]
    fn the_version_link_never_covers_the_report_button() {
        for width in [700.0, 900.0, 1400.0] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.app.custom_titlebar = true;
            h.call("ui.resize", json!({"width":width,"height":720}));
            h.frames(3);
            // A long version (a release candidate's) gives way to the document tabs on a narrow bar.
            let Some(version) = crate::scenario::handle_at("titlebar:version") else {
                assert!(width < 900.0, "the version is drawn as a link at {width}");
                continue;
            };
            let report = crate::scenario::handle_at("titlebar:report").expect("the report button is drawn");
            // Its click area starts where the report button ends, so a click on that button reaches the report.
            assert!(version.x > report.x, "width {width}");
        }
    }

    /// The desktop app shows the default header layout: search in the ribbon's tab row, a
    /// labelled Report a bug, the version at the bar's right end and not in the Help menu.
    #[test]
    fn the_default_header_layout_is_the_desktop_one() {
        assert_eq!(HeaderLayout::default(), HeaderLayout::DESKTOP);
        assert_eq!(
            HeaderLayout::DESKTOP,
            HeaderLayout { search: SearchPlace::Ribbon, report: ReportButton::Labelled, version: VersionPlace::Header }
        );
        let mut h = crate::scenario::Harness::new();
        assert_eq!(h.app.header, HeaderLayout::DESKTOP, "a new app has the desktop layout");
        h.app.home.open = false;
        h.app.custom_titlebar = true;
        h.call("ui.resize", json!({"width":1400.0,"height":720}));
        h.frames(3);
        let rect = |name: &str| crate::scenario::rect_of(name).unwrap_or_else(|| panic!("{name}"));
        let search = rect("ribbon:search");
        assert!(search.top() >= APP_BAR_HEIGHT, "search is in the ribbon: {search:?}");
        assert_eq!(search.height(), TAB_ROW_HEIGHT - 2.0 * ribbon::CARD_INSET);
        assert_eq!(search.width(), crate::ui_kit::CommandSearch::closed_width(&h.ctx, Some("S")));
        assert!(crate::scenario::rect_of("titlebar:search").is_none());
        let report = rect("titlebar:report");
        // A pre-release's longer version takes more room than RIGHT_GROUP and moves the button left.
        let t = Tokens::get();
        let version = format!("v{}", crate::update::current_version());
        let version = h.ctx.fonts_mut(|f| f.layout_no_wrap(version, FontId::proportional(t.font_small), t.app_bar_text).size().x);
        assert_eq!(report.right(), 1400.0 - crate::window_chrome::WIDTH - RIGHT_GROUP.max(12.0 + version + 8.0));
        assert_eq!(report.height(), 26.0);
        assert!(report.width() > 100.0, "Report a bug carries its label: {report:?}");
        assert_eq!(rect("titlebar:preferences").center().x, report.left() - 52.0);
        assert!(crate::scenario::handle_at("titlebar:version").is_some(), "the version is in the bar");
        h.step(&json!({"click":{"handle":"appbar:help"}})).unwrap();
        assert!(h.app.help.menu);
        let version = solidcraft_ui_text::format("help.version", &[("version", crate::update::current_version())]);
        assert!(h.widget("About SolidCraft").is_some());
        assert!(h.widget(&version).is_none(), "the Help menu has no version row");
    }

    /// The alternative header layout: search in the bar just left of Settings, Report a bug as an
    /// icon like Settings and Help, and the version as a Help menu row.
    #[test]
    fn a_host_can_move_search_report_and_version_in_the_header() {
        let mut h = crate::scenario::Harness::new();
        h.app.header = HeaderLayout::COMPACT;
        h.app.home.open = false;
        h.app.custom_titlebar = true;
        h.call("ui.resize", json!({"width":1400.0,"height":720}));
        h.frames(3);
        let rect = |name: &str| crate::scenario::rect_of(name).unwrap_or_else(|| panic!("{name}"));
        let (search, prefs) = (rect("titlebar:search"), rect("titlebar:preferences"));
        assert_eq!(search.right(), prefs.left() - HEADER_SEARCH_GAP, "search sits immediately left of Settings");
        assert!(search.bottom() <= APP_BAR_HEIGHT && (search.center().y - prefs.center().y).abs() < 0.5);
        assert_eq!(search.width(), crate::ui_kit::CommandSearch::closed_width(&h.ctx, Some("S")), "the whole field where there is room");
        assert_eq!(rect("ribbon:search"), search, "the one search field is drawn in the bar");
        // Report a bug: an icon the size of Settings and Help, still named for assistive tools.
        let report = rect("titlebar:report");
        assert_eq!(report.size(), vec2(32.0, 32.0));
        assert_eq!(report.right(), 1400.0 - crate::window_chrome::WIDTH - 8.0, "no room is kept for the version");
        assert_eq!(rect("appbar:help").size(), report.size());
        assert!(h.widget("Report a bug").is_some(), "the icon keeps its accessible name");
        assert!(crate::scenario::handle_at("titlebar:version").is_none(), "the version left the bar");
        // The ribbon's tabs run to its end.
        let card = rect("ribbon:card");
        assert!(rect("toolbar_tab:MODEL").left() < card.left() + 10.0);
        // Clicking the field opens command search.
        h.step(&json!({"click":{"handle":"titlebar:search"}})).unwrap();
        assert!(h.app.ui.palette_open, "the header field opens search");
        h.step(&json!({"key":"Escape"})).unwrap();
        assert!(!h.app.ui.palette_open);
        // The version is a Help menu row, right after About.
        h.step(&json!({"click":{"handle":"appbar:help"}})).unwrap();
        assert!(h.app.help.menu);
        let version = solidcraft_ui_text::format("help.version", &[("version", crate::update::current_version())]);
        let [_, about] = h.widget("About SolidCraft").unwrap();
        let [_, row] = h.widget(&version).expect("the Help menu lists the version");
        let [_, shortcuts] = h.widget("Keyboard Shortcuts").unwrap();
        assert!(about < row && row < shortcuts, "between About and Keyboard Shortcuts");
        h.step(&json!({"widget":version})).unwrap();
        assert!(!h.app.help.menu, "choosing the row closes the menu");
        // In a narrow bar the field folds to its magnifier and still opens search.
        h.call("ui.resize", json!({"width":700.0,"height":720}));
        h.frames(3);
        let search = rect("titlebar:search");
        assert_eq!(search.width(), crate::ui_kit::CommandSearch::compact_width(26.0));
        assert_eq!(search.right(), rect("titlebar:preferences").left() - HEADER_SEARCH_GAP);
        h.step(&json!({"click":{"handle":"titlebar:search"}})).unwrap();
        assert!(h.app.ui.palette_open, "the magnifier opens search");
    }

    /// With native menus the bar keeps its own Settings, so search the host put in the bar sits
    /// just left of it there too.
    #[test]
    fn search_in_the_bar_beside_native_menus() {
        let mut h = crate::scenario::Harness::new();
        h.app.header = HeaderLayout::COMPACT;
        h.app.native_file_menu = true;
        h.app.integrated_titlebar = true;
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1400.0,"height":720}));
        h.frames(3);
        let search = crate::scenario::rect_of("titlebar:search").expect("search in the bar");
        let gear = crate::scenario::rect_of("titlebar:preferences").expect("Settings in the bar");
        assert!(search.bottom() <= APP_BAR_HEIGHT && search.right() <= gear.left(), "{search:?} {gear:?}");
    }

    /// A release candidate's version is wider than a release's: the button moves left for it
    /// instead of lying under it, and where that would crowd out the document tabs the version is
    /// hidden, as on a narrow bar (#5621).
    #[test]
    fn a_longer_version_moves_the_report_button_instead_of_covering_it() {
        let h = crate::scenario::Harness::new();
        let t = Tokens::get();
        let measure = |text: &str| h.ctx.fonts_mut(|f| f.layout_no_wrap(text.into(), FontId::proportional(t.font_small), t.app_bar_text).size().x);
        let (release, candidate) = (measure("v0.25.1"), measure("v0.21.0-rc.1"));
        assert!(candidate > release + 15.0, "{release} {candidate}");
        for width in [700.0, 900.0, 1400.0] {
            let bar = Rect::from_min_size(egui::Pos2::ZERO, vec2(width, APP_BAR_HEIGHT));
            for captions in [0.0, crate::window_chrome::WIDTH] {
                // A release keeps the button where it always was.
                assert_eq!(version_room(bar, captions, 354.0, release), Some(RIGHT_GROUP), "{width} {captions}");
                for (k, version_width) in [release, candidate, 2.0 * candidate].into_iter().enumerate() {
                    let (report, _) = report_rect(bar, captions, 354.0, 80.0, version_width, HeaderLayout::DESKTOP);
                    let shown = version_room(bar, captions, 354.0, version_width).is_some();
                    if shown {
                        let version_left = bar.right() - 12.0 - captions - version_width;
                        assert!(report.right() + 8.0 <= version_left + 0.01, "{width} {captions} {version_width}: {report:?}");
                    } else {
                        assert_eq!(report.right(), bar.right() - captions - 8.0, "{width} {captions} {version_width}: the narrow bar's margin");
                    }
                    if k == 1 {
                        assert_eq!(shown, width >= 900.0, "{width} {captions}: the release candidate's version");
                    }
                }
            }
        }
    }

    #[test]
    fn header_report_actual_click_opens_capture_without_changing_healthy_design() {
        for width in [450.0, 700.0, 900.0, 1400.0] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.app.custom_titlebar = true;
            h.app.run("solid.box", json!({"length":35,"width":12,"height":4})).unwrap();
            h.call("ui.resize", json!({"width":width,"height":720}));
            h.frames(3);
            let before = (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.revision);
            let point = h.widget("Report a bug").expect("compact and full buttons retain an accessible name");
            h.step(&json!({"click":point})).unwrap();
            assert!(h.app.issue_report.draft.is_some(), "width {width}: header click must reach the report");
            assert!(crate::issue_report::pending(&h.app));
            assert_eq!(before, (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.revision));
            captured_frame(&mut h);
            // The report card takes a few frames to reach its full height.
            h.frames(16);
            let close_label = if h.widget("Close draft").is_some() { "Close draft" } else { "Close" };
            // Existing review content scrolls vertically when the window is narrow.
            if width < 700.0 {
                let [x, y] = h.widget(close_label).unwrap();
                h.call("ui.scroll", json!({"x":x,"y":y - 60.0,"delta":-1200}));
                // Let the scroll animation finish before locating the click target.
                h.frames(60);
            }
            h.step(&json!({"widget":close_label})).unwrap();
            assert!(h.app.issue_report.draft.is_none(), "width {width}: review closes through its actual button");
            h.step(&json!({"click":{"handle":"tab:home"}})).unwrap();
            assert!(h.app.home.open, "width {width}: the Home tab remains reachable beside Report");
            assert_eq!(before, (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.revision));
        }
    }

    #[test]
    fn header_report_preserves_new_dimension_draft_and_single_commit() {
        crate::dim_view::cancel();
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.line", json!({"points":[[0,0],[20,0]]})).unwrap();
        h.call("ui.view", json!({"view":"top"}));
        h.app.cam.distance = 80.0;
        h.app.cam.target = solidcore_record::Vec3::new(10.0, 0.0, 0.0);
        h.settle();
        h.step(&json!({"start":"sketch.dimension"})).unwrap();
        for point in [[0, 0, 0], [20, 0, 0], [10, 5, 0]] {
            h.step(&json!({"click":{"world":point}})).unwrap();
        }
        h.step(&json!({"text":"30 mm"})).unwrap();
        let before = (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.revision);
        h.step(&json!({"widget":"Report a bug"})).unwrap();
        h.frames(3);
        assert!(h.app.issue_report.draft.is_some() && crate::dim_view::editing());
        captured_frame(&mut h);
        h.ctx.memory_mut(|m| m.request_focus(egui::Id::unique("issue_report_body")));
        h.step(&json!({"text":"Describe this dimension problem."})).unwrap();
        h.step(&json!({"key":"Enter"})).unwrap();
        assert_eq!(before, (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.revision));
        h.step(&json!({"widget":"Close draft"})).unwrap();
        h.frames(2);
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(egui::Id::unique("sc_dim_edit")));
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(!crate::dim_view::editing());
        assert_eq!(h.app.session.undo.len(), before.1 + 1);
        assert_eq!(h.app.session.doc.param("d1").unwrap().expr, "30 mm");
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), before.0);
    }
}

#[cfg(test)]
mod home_tests {
    use super::*;

    /// Home hides the design: its Undo and Redo buttons, the File menu's Insert, Save
    /// and Export, and CAD keyboard shortcuts leave it alone. File-view Save shortcuts
    /// have their own regression coverage; viewport actions work again after returning.
    #[test]
    fn design_actions_are_off_on_home() {
        // A design with history both ways, a selection and unsaved changes, behind Home.
        let on_home = || {
            let mut h = crate::scenario::Harness::new();
            h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Main"})).unwrap();
            h.app.run("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [20, 0, 0]})).unwrap();
            h.app.run("edit.undo", json!({})).unwrap();
            h.app.session.selection = vec![solidcraft_engine::Sel::Body { name: "Main".into() }];
            let rev = h.app.session.revision;
            h.app.home.show(rev);
            h.frames(2);
            h
        };
        let mut h = on_home();
        assert!(!design_shown(&h.app));
        // Everything an action could change: the design, its history and file, files written,
        // the selection and visibility, and whether it left Home or opened a tool, dialog or the command box.
        let state = |h: &crate::scenario::Harness| {
            let files = std::fs::read_dir(&h.dir).map_or(0, |d| d.count());
            let doc = serde_json::to_value(&*h.app.session.doc).unwrap();
            let busy = (h.app.tool.is_some(), h.app.dialog.is_some(), h.app.ui.palette_open);
            (
                doc,
                h.app.session.undo.len(),
                h.app.session.redo.len(),
                h.app.session.path.clone(),
                files,
                h.app.session.selection.clone(),
                h.app.session.visibility.hidden_bodies.clone(),
                h.app.home.open,
                busy,
            )
        };
        let click = |h: &mut crate::scenario::Harness, handle: &str| {
            let at = crate::scenario::handle_at(handle).unwrap();
            h.call("ui.click", json!({"x": at.x, "y": at.y}));
        };
        let mut actions: Vec<(String, Box<dyn Fn(&mut crate::scenario::Harness)>)> = Vec::new();
        for icon in ["undo", "redo"] {
            actions.push((format!("{icon} button"), Box::new(move |h| click(h, &format!("appbar:{icon}")))));
        }
        for (key, cmd) in [("Z", true), ("Y", true), ("Delete", false), ("V", false), ("E", false), ("S", false)] {
            let name = if cmd { format!("Ctrl+{key}") } else { key.to_string() };
            actions.push((name, Box::new(move |h| drop(h.call("ui.key", json!({"key": key, "cmd": cmd}))))));
        }
        for item in ["Insert STEP…", "Insert Mesh…", "Save", "Save As…", "Export STL…", "Export STEP…", "Export 3MF…", "Export OBJ…", "Export URDF…"]
        {
            actions.push((
                format!("File › {item}"),
                Box::new(move |h| {
                    h.ctx.data_mut(|d| d.insert_temp(egui::Id::unique("sc_file_menu"), false));
                    click(h, "appbar:folder");
                    // A same-named File navigation action is now present: target the menu row explicitly.
                    if let Some(at) = crate::scenario::handle_at(&format!("menu:{item}")) {
                        h.call("ui.click", json!({"x":at.x,"y":at.y}));
                    }
                }),
            ));
        }
        // Each action on its own design, so one cannot hide another.
        let mut acted = Vec::new();
        for (name, act) in &actions {
            let mut h = on_home();
            let before = state(&h);
            act(&mut h);
            h.settle();
            if state(&h) != before {
                acted.push(name.clone());
            }
        }
        assert!(acted.is_empty(), "these acted on the hidden design from Home: {acted:?}");

        h.app.home.open = false;
        h.frames(2);
        let at = crate::scenario::handle_at("appbar:redo").unwrap();
        h.call("ui.click", json!({"x": at.x, "y": at.y}));
        assert_eq!(h.app.session.redo.len(), 0, "Redo works once Home is closed");
        h.call("ui.key", json!({"key": "Z", "cmd": true}));
        h.settle();
        assert_eq!(h.app.session.redo.len(), 1, "Ctrl+Z works once Home is closed");
        h.ctx.data_mut(|d| d.insert_temp(egui::Id::unique("sc_file_menu"), false));
        click(&mut h, "appbar:file");
        h.step(&json!({"widget": "Save"})).unwrap();
        h.settle();
        assert!(h.app.session.path.is_some() && !h.app.session.is_dirty(), "the design saves once Home is closed");
    }

    /// File › Export URDF… writes the robot description and its meshes next to the picked file.
    #[test]
    fn file_menu_exports_a_urdf() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        h.frames(2);
        h.ctx.data_mut(|d| d.insert_temp(egui::Id::unique("sc_file_menu"), false));
        let at = crate::scenario::handle_at("appbar:folder").unwrap();
        h.call("ui.click", json!({"x": at.x, "y": at.y}));
        let _ = h.step(&json!({"widget": "Export URDF…"}));
        h.settle();
        let files: Vec<String> = std::fs::read_dir(&h.dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert!(files.iter().any(|f| f.ends_with(".urdf")) && files.iter().any(|f| f.ends_with("-base.stl")), "{files:?}");
    }

    /// The disabled buttons' tooltip is translated in every catalog (not English fallback).
    #[test]
    fn needs_design_tooltip_is_translated() {
        crate::i18n::install();
        let english = solidcraft_ui_text::Locale::for_language("en").text("ui.needs_design");
        assert!(english.contains("{command}"));
        for catalog in crate::i18n::LOCALES.iter().filter(|c| !matches!(c.id, "en" | "en-GB")) {
            let text = solidcraft_ui_text::Locale::for_language(catalog.id).text("ui.needs_design");
            assert!(text != english && text.contains("{command}"), "{} must translate ui.needs_design", catalog.id);
        }
    }
}

#[cfg(test)]
mod dropdown_tests {
    use super::*;

    #[test]
    fn sketch_create_families_have_icons_and_open_their_variants() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.frames(4);
        h.step(&json!({"click":{"handle":"panel:SKETCH:CREATE"}})).unwrap();
        h.frames(4);
        let family = crate::scenario::rect_of("menu:Line").expect("Line is a shared menu row");
        assert!(family.contains(crate::scenario::handle_at("create_family:Line").unwrap()));
        h.step(&json!({"click":{"handle":"create_family:Line"}})).unwrap();
        h.frames(4);
        for id in ["sketch.line", "sketch.line.midpoint"] {
            assert!(crate::scenario::handle_at(&format!("dropdown:{id}")).is_some(), "missing variant {id}");
        }
        h.step(&json!({"click":{"handle":"dropdown:sketch.line.midpoint"}})).unwrap();
        assert_eq!(h.app.tool.as_ref().map(|tool| tool.cmd), Some("sketch.line.midpoint"));
    }

    /// Plain rows (Point) and family rows (Slot) span the same width, so a plain row's hover
    /// background and pin reach the drop-down's right edge.
    #[test]
    fn sketch_create_plain_and_family_rows_share_both_edges() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.frames(4);
        h.step(&json!({"click":{"handle":"panel:SKETCH:CREATE"}})).unwrap();
        h.frames(4);
        let point = crate::scenario::rect_of("menu:sketch.point").expect("Point is a plain row");
        let slot = crate::scenario::rect_of("menu:Slot").expect("Slot is a family row");
        assert!((point.left() - slot.left()).abs() < 0.5, "left edges differ: Point {point:?}, Slot {slot:?}");
        assert!((point.right() - slot.right()).abs() < 0.5, "right edges differ: Point {point:?}, Slot {slot:?}");
    }

    #[test]
    fn sketch_create_family_rows_paint_the_first_variants_icons() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        let cmds: Vec<_> = solidcraft_engine::command_specs().into_iter().filter(|c| c.tab == "SKETCH" && c.panel == "CREATE").collect();
        crate::icon_svg::audit::start();
        let mut output = h.ctx.run_ui(egui::RawInput::default(), |ui| {
            sketch_create_menu(&mut h.app, ui, &cmds, 200.0, &mut None, &mut None);
        });
        output.textures_delta.clear();
        let requested = crate::icon_svg::audit::take();
        crate::icon_svg::audit::stop();
        for family in crate::sketch_families::FAMILIES {
            let first = solidcraft_engine::find_command(family.commands[0]).unwrap();
            assert!(!first.icon.is_empty());
            assert!(requested.iter().any(|icon| icon.name == first.icon && !icon.placeholder), "{} must paint {}", family.label, first.icon);
        }
    }

    /// A drop-down taller than the window stops at its bottom and scrolls: the last row is
    /// reached with the arrow keys and comes into view.
    #[test]
    fn a_tall_drop_down_scrolls_inside_a_short_window() {
        for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.set", json!({"layout":mode}));
            h.call("ui.resize", json!({"width":1200,"height":320}));
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            h.frames(4);
            h.step(&json!({"click":{"handle":"panel:SKETCH:CREATE"}})).unwrap();
            h.frames(4);
            let menu = crate::scenario::rect_of("toolbar_dropdown").expect("the drop-down is open");
            assert!(menu.bottom() <= 320.0, "the drop-down runs past the window: {menu:?}");
            assert!(crate::scenario::handle_at("dropdown:sketch.text").is_some_and(|p| !menu.contains(p)), "Text starts below the fold");
            // Up from no focus goes to the last enabled row; it scrolls into view.
            h.step(&json!({"key":"ArrowUp"})).unwrap();
            h.frames(10);
            let hot = crate::scenario::handles("dropdown_hot:");
            let [(_, at)] = hot.as_slice() else { panic!("one highlighted row: {hot:?}") };
            let menu = crate::scenario::rect_of("toolbar_dropdown").unwrap();
            assert!(menu.contains(*at), "the focused last row is out of view: {at:?} in {menu:?}");
        }
    }

    /// Every row of a drop-down is shown when the window has room, even after a shorter list was
    /// shown first: another tab's panel of the same name, or the same panel collapsed (#874).
    #[test]
    fn a_drop_down_shows_all_its_rows_after_a_shorter_one() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1600,"height":1000}));
        let open = |h: &mut crate::scenario::Harness, tab: &str, panel: &str| {
            h.app.ui.tab = tab.into();
            h.frames(4);
            h.step(&json!({"click":{"handle":format!("panel:{tab}:{panel}")}})).unwrap();
            h.frames(6);
            let menu = crate::scenario::rect_of("toolbar_dropdown").expect("the drop-down is open");
            let rows = crate::scenario::handles("dropdown:");
            h.step(&json!({"key":"Escape"})).unwrap();
            h.frames(4);
            (menu, rows)
        };
        let (short, short_rows) = open(&mut h, "MANAGE", "RULES");
        let (menu, rows) = open(&mut h, "MODEL", "MODIFY");
        assert!(rows.len() > short_rows.len(), "MODEL MODIFY lists more than MANAGE RULES: {rows:?} vs {short_rows:?}");
        assert!(menu.height() > short.height(), "the longer list is taller: {menu:?} vs {short:?}");
        for (id, at) in &rows {
            assert!(menu.contains(*at), "{id} at {at:?} is cut off by the drop-down {menu:?}");
        }
    }

    /// Both layouts keep the original order by default and expose every tool with grouping on.
    #[test]
    fn construct_dropdown_groups_are_visible_in_list_and_grid() {
        for (grid, grouped) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.app.preferences.ribbon_icon_grid = grid;
            h.app.preferences.construct_menu_groups = grouped;
            h.call("ui.resize", json!({"width":1600,"height":1200}));
            h.app.ui.tab = "MODEL".into();
            h.frames(4);
            h.step(&json!({"click":{"handle":"panel:MODEL:CONSTRUCT"}})).unwrap();
            h.frames(6);
            let popup = h.ctx.data(|d| d.get_temp::<egui::Id>(egui::Id::unique("sc_panel_open"))).unwrap();
            h.ctx = egui::Context::default();
            crate::fonts::setup_bundled_fonts(&h.ctx);
            h.ctx.data_mut(|d| d.insert_temp(egui::Id::unique("sc_panel_open"), popup));
            let mut textures = std::collections::HashMap::new();
            let mut output = None;
            for frame in 0..12 {
                let mut drawn = h.ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1600., 1200.))),
                        time: Some(frame as f64 / 60.),
                        ..Default::default()
                    },
                    |ui| h.app.ui(ui),
                );
                crate::ui_kit::test_textures_update(&mut textures, &drawn.textures_delta);
                drawn.textures_delta.clear();
                output = Some(drawn);
            }
            let output = output.unwrap();
            if let Ok(dir) = std::env::var("SOLIDCRAFT_CONSTRUCT_EVIDENCE") {
                let bounds = crate::scenario::rect_of("toolbar_dropdown").unwrap().expand(8.);
                let image = crate::ui_kit::test_raster(&h.ctx, output.shapes.clone(), output.pixels_per_point, bounds, &textures);
                std::fs::create_dir_all(&dir).unwrap();
                image
                    .save(std::path::Path::new(&dir).join(format!(
                        "construct-{}-{}.png",
                        if grid { "grid" } else { "list" },
                        if grouped { "grouped" } else { "default" }
                    )))
                    .unwrap();
            }
            for heading in ["Planes", "Axes", "Points"] {
                assert_eq!(
                    output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == heading)),
                    grouped,
                    "{heading} in grid={grid}, grouped={grouped}"
                );
            }
            let (cmds, _) = crate::workspace::layout("MODEL", "CONSTRUCT", &solidcraft_engine::command_specs()).unwrap();
            let prefix = "dropdown:";
            if !grouped {
                let mut positions: Vec<_> = cmds.iter().map(|c| (c.id, crate::scenario::handle_at(&format!("{prefix}{}", c.id)).unwrap())).collect();
                positions.sort_by(|a, b| a.1.y.total_cmp(&b.1.y).then(a.1.x.total_cmp(&b.1.x)));
                assert_eq!(
                    positions.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
                    cmds.iter().map(|c| c.id).collect::<Vec<_>>(),
                    "original order in grid={grid}"
                );
            }
            for c in &cmds {
                assert!(crate::scenario::handle_at(&format!("{prefix}{}", c.id)).is_some(), "{} remains reachable in grid={grid}", c.id);
            }
            h.step(&json!({"click":{"handle":format!("{prefix}construct.plane.offset")}})).unwrap();
            assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::OffsetPlane { .. })));
        }
    }

    /// The grid's rows: a heading starts a new one, and the arrow keys move along and between
    /// them over the enabled icons only, wrapping at the ends (#4739).
    #[test]
    fn grid_rows_break_at_headings_and_arrow_keys_skip_disabled_icons() {
        let c = |id| solidcraft_engine::find_command(id).unwrap();
        let grouped = [
            (Some("Primitives"), c("solid.box")),
            (None, c("solid.cylinder")),
            (None, c("solid.sphere")),
            (None, c("solid.torus")),
            (None, c("solid.pipe")),
            (Some("Other"), c("solid.extrude")),
            (None, c("solid.revolve")),
        ];
        // Too narrow for the groups side by side (two icons wide): rows as wide as the window, a
        // heading starting a row.
        let rows = ribbon::grid_layout(&grid_ids(&grouped), &[], 100.0);
        let at = |l: &ribbon::GridLayout| l.cells.iter().map(|(_, at)| *at).collect::<Vec<_>>();
        assert_eq!(at(&rows), [(0, 0), (0, 1), (1, 0), (1, 1), (2, 0), (3, 0), (3, 1)]);
        assert_eq!(rows.headings.len(), 2);
        // Room enough: each group a column of two under its heading, the groups side by side.
        let side = ribbon::grid_layout(&grid_ids(&grouped), &[], 1000.0);
        assert_eq!(at(&side), [(0, 0), (0, 1), (1, 0), (1, 1), (2, 0), (0, 2), (0, 3)]);
        let headings: Vec<_> = side.headings.iter().map(|(p, _, h)| (p.x, *h)).collect();
        assert_eq!(headings, [(0.0, "Primitives"), (2.0 * ribbon::GRID_CELL, "Other")]);
        assert_eq!(side.size, vec2(4.0 * ribbon::GRID_CELL, ribbon::GRID_HEADING + 3.0 * ribbon::GRID_CELL));
        assert_eq!(side.separators, [2.0 * ribbon::GRID_CELL], "a line between the two groups");
        assert!(rows.separators.is_empty(), "rows have none");
        assert!(side.cells.iter().all(|(r, _)| r.top() >= ribbon::GRID_HEADING), "icons below the headings");
        // A menu without groups keeps its rows of five.
        let plain: Vec<_> = grouped.iter().map(|(_, c)| (None, *c)).collect();
        assert_eq!(at(&ribbon::grid_layout(&grid_ids(&plain), &[], 1000.0)), [(0, 0), (0, 1), (0, 2), (0, 3), (0, 4), (1, 0), (1, 1)]);
        let cells = at(&side);
        // `solid.pipe` (index 4) is disabled: `enabled` holds the others.
        let enabled = [0, 1, 2, 3, 5, 6];
        let step = |at, key| ribbon::grid_step(&cells, &enabled, at, key).map(|k: usize| enabled[k]);
        assert_eq!((step(None, ribbon::GridKey::Right), step(None, ribbon::GridKey::Down)), (Some(0), Some(0)));
        assert_eq!((step(None, ribbon::GridKey::Left), step(None, ribbon::GridKey::Up)), (Some(6), Some(6)));
        assert_eq!(step(Some(2), ribbon::GridKey::Right), Some(3), "Right runs on into the next row");
        assert_eq!(step(Some(0), ribbon::GridKey::Left), Some(6), "and wraps");
        assert_eq!(step(Some(1), ribbon::GridKey::Down), Some(3), "Down goes to the nearest enabled icon below");
        assert_eq!(step(Some(4), ribbon::GridKey::Down), Some(3), "from the next group's column too");
        assert_eq!(step(Some(2), ribbon::GridKey::Down), Some(0), "from the last enabled row Down wraps to the first");
        assert_eq!(step(Some(5), ribbon::GridKey::Up), Some(3), "from the first row Up wraps to the last, nearest column");
        assert_eq!(step(Some(1), ribbon::GridKey::Up), Some(3));
        assert_eq!(ribbon::grid_step(&cells, &[], None, ribbon::GridKey::Down), None);
    }

    /// A command in no group (Model › Modify's Delete) stands apart after the groups, without a
    /// heading: leaving out a pinned group (Specialise's Convert to Sheet Metal) must not move it
    /// under the group before (Bodies) (#4739).
    #[test]
    fn a_command_in_no_group_stays_apart_when_a_group_is_left_out() {
        let specs = solidcraft_engine::command_specs();
        let (cmds, _) = crate::workspace::layout("MODEL", "MODIFY", &specs).unwrap();
        let loose = crate::workspace::ungrouped("MODEL", "MODIFY", &cmds);
        assert_eq!(loose, ["timeline.delete"]);
        let column_of = |cmds: &[&'static solidcraft_engine::CommandSpec], id: &str| {
            let grouped = crate::workspace::grouped_commands("MODEL", "MODIFY", cmds);
            let layout = ribbon::grid_layout(&grid_ids(&grouped), &loose, 4000.0);
            let x = |id: &str| grouped.iter().position(|(_, c)| c.id == id).and_then(|i| layout.cells.get(i)).map(|(r, _)| r.left());
            let last_group = layout.separators.last().copied();
            (x(id), last_group)
        };
        let (delete, separator) = column_of(&cmds, "timeline.delete");
        assert_eq!(delete, separator, "Delete in a column of its own after Specialise");
        let skipped = skip_on_ribbon(cmds.clone(), &["sheet.convert"]);
        let (delete, separator) = column_of(&skipped, "timeline.delete");
        let (remove, _) = column_of(&skipped, "solid.remove");
        assert_eq!(delete, separator, "still apart without Specialise");
        assert!(delete > remove, "not under Bodies: {delete:?} vs {remove:?}");
    }

    /// Group headings sit below the drop-down's own header in the hierarchy: a smaller font than
    /// the header's, and written as named rather than in capitals (list and grid alike).
    #[test]
    fn group_headings_are_smaller_than_the_drop_down_header() {
        let t = Tokens::get();
        assert!(ribbon::heading_font().size < t.font_caption, "{} vs the header's {}", ribbon::heading_font().size, t.font_caption);
        assert_eq!(group_heading_text("Edges"), "Edges", "written as named, not in capitals");
    }

    /// Every panel's drop-down of every tab, and Sketch › Create's families, is at most
    /// ribbon::GRID_ROWS high in the grid when the window is wide enough: big menus grow to the right.
    #[test]
    fn every_grid_menu_is_at_most_three_rows_high() {
        let rows = |l: &ribbon::GridLayout| l.cells.iter().map(|(r, _)| r.top().round() as i32).collect::<std::collections::BTreeSet<_>>().len();
        let tabs: std::collections::BTreeSet<&str> = crate::workspace::SECTIONS.iter().map(|s| s.tab).collect();
        for tab in tabs {
            for (panel, cmds, _) in crate::toolbar_custom::panel_lists(tab) {
                let grouped = crate::workspace::grouped_commands(tab, panel, &cmds);
                let layout = ribbon::grid_layout(&grid_ids(&grouped), &crate::workspace::ungrouped(tab, panel, &cmds), 4000.0);
                assert!(rows(&layout) <= ribbon::GRID_ROWS, "{tab} {panel}: {} rows", rows(&layout));
                if tab == "SKETCH" && panel == "CREATE" {
                    let families = ribbon::grid_layout(&grid_ids(&sketch_create_grouped(&cmds)), &[], 4000.0);
                    assert!(rows(&families) <= ribbon::GRID_ROWS, "Sketch › Create families: {} rows", rows(&families));
                }
            }
        }
        // A window too narrow for the width wraps into more rows instead of running off it.
        let c = |id| solidcraft_engine::find_command(id).unwrap();
        let many: Vec<_> = solidcraft_engine::command_specs().into_iter().take(30).map(|c| (None, c)).collect();
        let narrow = ribbon::grid_layout(&grid_ids(&many), &[], 4.0 * ribbon::GRID_CELL);
        assert!(narrow.size.x <= 4.0 * ribbon::GRID_CELL && rows(&narrow) > ribbon::GRID_ROWS);
        let few = [(None, c("solid.box")), (None, c("solid.sphere"))];
        assert_eq!(
            ribbon::grid_layout(&grid_ids(&few), &[], 4000.0).size,
            vec2(2.0 * ribbon::GRID_CELL, ribbon::GRID_CELL),
            "a small menu is one row"
        );
    }

    /// With `ribbon_icon_grid` on, a panel's drop-down shows the same commands as its list, as
    /// large icons in rows of at most five; arrow keys move across and down; a click starts the
    /// command, and a drag pins it, as from the list (#4739).
    #[test]
    fn the_icon_grid_shows_the_lists_commands_and_acts_like_it() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1600,"height":1000}));
        let open = |h: &mut crate::scenario::Harness| {
            h.frames(4);
            h.step(&json!({"click":{"handle":"panel:MODEL:CREATE"}})).unwrap();
            h.frames(6);
            let menu = crate::scenario::rect_of("toolbar_dropdown").expect("the drop-down is open");
            (menu, crate::scenario::handles("dropdown:"))
        };
        let (_, list) = open(&mut h);
        h.step(&json!({"key":"Escape"})).unwrap();
        assert!(!h.app.preferences.ribbon_icon_grid, "the list stays the default");
        h.call("ui.prefs", json!({"ribbon_icon_grid": true}));
        assert!(h.app.preferences.ribbon_icon_grid);
        let (menu, grid) = open(&mut h);
        let ids = |rows: &[(String, egui::Pos2)]| rows.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&grid), ids(&list), "the grid shows the list's commands in its order");
        assert!(grid.len() > ribbon::GRID_COLUMNS * ribbon::GRID_ROWS, "MODEL CREATE is a big menu: {grid:?}");
        for (id, at) in &grid {
            assert!(menu.contains(*at), "{id} at {at:?} is outside the drop-down {menu:?}");
        }
        let ys: std::collections::BTreeSet<i32> = grid.iter().map(|(_, at)| at.y.round() as i32).collect();
        assert_eq!(ys.len(), ribbon::GRID_ROWS, "laid out in {} rows, growing to the right: {ys:?}", ribbon::GRID_ROWS);
        let columns = grid.len().div_ceil(ribbon::GRID_ROWS);
        assert!(menu.width() <= columns as f32 * ribbon::GRID_CELL + 40.0, "no wider than its columns: {menu:?}");

        let hot = |h: &mut crate::scenario::Harness, key: &str| {
            h.step(&json!({"key": key})).unwrap();
            h.frames(3);
            let hot = crate::scenario::handles("dropdown_hot:");
            let [(_, at)] = hot.as_slice() else { panic!("one highlighted icon after {key}: {hot:?}") };
            *at
        };
        let first = hot(&mut h, "ArrowRight");
        let second = hot(&mut h, "ArrowRight");
        assert!(second.x > first.x && (second.y - first.y).abs() < 1.0, "Right moves along the row: {first:?} → {second:?}");
        let below = hot(&mut h, "ArrowDown");
        assert!(below.y > second.y, "Down moves to the row below: {second:?} → {below:?}");

        h.step(&json!({"click":{"handle":"dropdown:solid.box"}})).unwrap();
        h.frames(3);
        assert!(crate::scenario::rect_of("toolbar_dropdown").is_none(), "a click closes the drop-down");
        assert!(h.app.dialog.is_some() || h.app.tool.is_some(), "and starts the command");
        h.step(&json!({"key":"Escape"})).unwrap();
        h.frames(3);

        open(&mut h);
        let label = crate::scenario::handle_at("panel:MODEL:CREATE").unwrap();
        h.step(&json!({"drag":[{"handle":"dropdown:solid.cylinder"}, [label.x, label.y]],"steps":12})).unwrap();
        h.step(&json!({"key":"Escape"})).unwrap();
        assert!(crate::toolbar_custom::pinned_on(&h.app, "MODEL", "solid.cylinder").is_some(), "dragged onto the ribbon, it is pinned");
        assert!(h.app.session.doc.features.is_empty(), "pinning does not run the command");
    }

    /// A right-click on a drop-down's command, in the list and in the grid, opens its menu:
    /// Pin to Toolbar, Remove from Toolbar and Pin to Shortcuts act on that command.
    #[test]
    fn a_right_click_in_the_drop_down_pins_and_unpins() {
        for grid in [false, true] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", json!({"width":1600,"height":1000}));
            h.app.preferences.ribbon_icon_grid = grid;
            let pick = |h: &mut crate::scenario::Harness, item: &str| {
                h.frames(4);
                h.step(&json!({"click":{"handle":"panel:MODEL:CREATE"}})).unwrap();
                h.frames(4);
                h.step(&json!({"click":{"handle":"dropdown:solid.box"},"button":"right"})).unwrap();
                h.frames(4);
                assert!(h.app.viewport.context_menu.is_some(), "grid {grid}: the right-click opens the command's menu");
                h.step(&json!({"widget": item})).unwrap_or_else(|e| panic!("grid {grid}: {item}: {e}"));
                h.frames(4);
            };
            pick(&mut h, "Pin to Toolbar");
            assert!(crate::toolbar_custom::pinned_on(&h.app, "MODEL", "solid.box").is_some(), "grid {grid}: pinned");
            pick(&mut h, "Remove from Toolbar");
            assert!(crate::toolbar_custom::pinned_on(&h.app, "MODEL", "solid.box").is_none(), "grid {grid}: unpinned");
            pick(&mut h, "Pin to Shortcuts");
            assert!(h.app.sbox.pinned.iter().any(|p| p == "solid.box"), "grid {grid}: a shortcut");
            // The pin shown on the pointed-at row or icon pins it, and again unpins it. A grid
            // icon's pin takes clicks only after the pointer has rested on it for ribbon::PIN_DELAY.
            for pinned in [true, false] {
                h.step(&json!({"click":{"handle":"panel:MODEL:CREATE"}})).unwrap();
                h.frames(4);
                h.step(&json!({"move":{"handle":"dropdown:solid.box"}})).unwrap();
                h.frames(3);
                if grid {
                    assert!(crate::scenario::handle_at("dropdown_pin:solid.box").is_none(), "no pin to click straight away");
                    h.frames((ribbon::PIN_DELAY * 60.0) as usize + 5);
                }
                h.step(&json!({"click":{"handle":"dropdown_pin:solid.box"}})).unwrap();
                h.frames(3);
                h.step(&json!({"key":"Escape"})).unwrap();
                assert_eq!(crate::toolbar_custom::pinned_on(&h.app, "MODEL", "solid.box").is_some(), pinned, "grid {grid}: the hover pin");
            }
            assert!(h.app.session.doc.features.is_empty(), "grid {grid}: pinning does not run the command");
        }
    }

    /// A quick click on a grid icon's corner, where its pin appears after ribbon::PIN_DELAY, starts the
    /// command instead of pinning it.
    #[test]
    fn a_quick_click_on_the_pin_corner_starts_the_command() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1600,"height":1000}));
        h.app.preferences.ribbon_icon_grid = true;
        h.frames(4);
        h.step(&json!({"click":{"handle":"panel:MODEL:CREATE"}})).unwrap();
        h.frames(4);
        let at = crate::scenario::handle_at("dropdown:solid.box").unwrap() + vec2(ribbon::GRID_CELL / 2.0 - 7.0, -ribbon::GRID_CELL / 2.0 + 7.0);
        h.step(&json!({"click":[at.x, at.y]})).unwrap();
        h.frames(3);
        assert!(crate::toolbar_custom::pinned_on(&h.app, "MODEL", "solid.box").is_none(), "not pinned");
        assert!(h.app.dialog.is_some() || h.app.tool.is_some(), "the command starts");
    }

    /// A command pinned from the grid becomes a ribbon button; Remove from Toolbar in the
    /// button's own right-click menu takes it off again, with both options on.
    #[test]
    fn a_pinned_button_is_removed_from_its_own_menu() {
        for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
            a_pinned_button_is_removed_in(mode);
        }
    }

    fn a_pinned_button_is_removed_in(mode: crate::ui_kit::LayoutMode) {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.set", json!({"layout":mode}));
        h.call("ui.resize", json!({"width":1600,"height":1000}));
        h.app.preferences.ribbon_icon_grid = true;
        h.app.preferences.ribbon_menus_skip_pinned = true;
        h.frames(4);
        h.step(&json!({"click":{"handle":"panel:MODEL:CREATE"}})).unwrap();
        h.frames(4);
        h.step(&json!({"move":{"handle":"dropdown:solid.loft"}})).unwrap();
        h.frames((ribbon::PIN_DELAY * 60.0) as usize + 5);
        h.step(&json!({"click":{"handle":"dropdown_pin:solid.loft"}})).unwrap();
        h.frames(3);
        h.step(&json!({"key":"Escape"})).unwrap();
        h.frames(4);
        assert!(crate::scenario::handle_at("button:solid.loft").is_some(), "pinned, Loft is a ribbon button");
        h.step(&json!({"click":{"handle":"button:solid.loft"},"button":"right"})).unwrap();
        h.frames(4);
        assert!(h.app.viewport.context_menu.is_some(), "the button's menu opens");
        h.step(&json!({"widget":"Remove from Toolbar"})).unwrap();
        h.frames(4);
        assert!(crate::toolbar_custom::pinned_on(&h.app, "MODEL", "solid.loft").is_none(), "removed: {:?}", h.app.toolbar_custom.panels);
        assert!(crate::scenario::handle_at("button:solid.loft").is_none(), "and gone from the ribbon");
    }

    /// Sketch › Create in the grid shows every variant of each family (Line, Rectangle, …) under
    /// the family's heading, then the other tools, as large icons; a click starts the tool.
    #[test]
    fn sketch_create_in_the_grid_shows_every_variant() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1600,"height":1000}));
        h.app.preferences.ribbon_icon_grid = true;
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.frames(4);
        h.step(&json!({"click":{"handle":"panel:SKETCH:CREATE"}})).unwrap();
        h.frames(4);
        let cells: Vec<String> = crate::scenario::handles("dropdown:").into_iter().map(|(id, _)| id).collect();
        for id in ["sketch.line", "sketch.rectangle.center", "sketch.circle.three_point", "sketch.arc.tangent", "sketch.polygon.edge", "sketch.point"]
        {
            assert!(cells.iter().any(|c| *c == format!("dropdown:{id}")), "{id} is an icon of the grid: {cells:?}");
        }
        let grouped = sketch_create_grouped(
            &solidcraft_engine::command_specs().into_iter().filter(|c| c.id.starts_with("sketch.rectangle")).collect::<Vec<_>>(),
        );
        assert_eq!(grouped.first().and_then(|(h, _)| *h), Some("Rectangle"));
        assert!(grouped.iter().skip(1).all(|(h, _)| h.is_none()), "one heading per family");
        h.step(&json!({"click":{"handle":"dropdown:sketch.rectangle.center"}})).unwrap();
        h.frames(3);
        assert!(crate::scenario::rect_of("toolbar_dropdown").is_none(), "a click closes the drop-down");
        assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some("sketch.rectangle.center"), "and starts the tool");
    }

    /// With `ribbon_menus_skip_pinned`, a drop-down leaves out the commands the ribbon shows as
    /// buttons, in the list and in the grid; a panel too narrow for its buttons lists them again.
    #[test]
    fn pinned_commands_can_be_left_out_of_the_drop_down() {
        for grid in [false, true] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", json!({"width":1600,"height":1000}));
            h.app.preferences.ribbon_icon_grid = grid;
            let rows = |h: &mut crate::scenario::Harness, panel: &str| {
                h.frames(4);
                h.step(&json!({"click":{"handle":panel}})).unwrap();
                h.frames(4);
                let rows: Vec<String> = crate::scenario::handles("dropdown:").into_iter().map(|(id, _)| id).collect();
                h.step(&json!({"key":"Escape"})).unwrap();
                rows
            };
            let all = rows(&mut h, "panel:MODEL:CREATE");
            assert!(all.iter().any(|id| id == "dropdown:solid.extrude"), "grid {grid}: listed by default: {all:?}");
            h.call("ui.prefs", json!({"ribbon_menus_skip_pinned": true}));
            let rest = rows(&mut h, "panel:MODEL:CREATE");
            for pinned in ["solid.extrude", "solid.revolve", "solid.hole"] {
                assert!(!rest.iter().any(|id| *id == format!("dropdown:{pinned}")), "grid {grid}: {pinned} is on the ribbon: {rest:?}");
            }
            assert_eq!(rest.len() + 3, all.len(), "grid {grid}: only the ribbon's buttons are left out");
            // In a window too narrow for the Extrude button, MODEL CREATE's menu lists it again.
            let narrow = (3..=16).rev().map(|w| w as f32 * 100.0).find(|w| {
                h.call("ui.resize", json!({"width": w, "height": 1000}));
                h.frames(4);
                crate::scenario::handle_at("button:solid.extrude").is_none()
            });
            assert!(narrow.is_some(), "grid {grid}: a narrow window has no room for the Extrude button");
            let panel = ["panel:MODEL:CREATE", "collapsed:MODEL:CREATE"].into_iter().find(|p| crate::scenario::handle_at(p).is_some());
            let Some(panel) = panel else { panic!("grid {grid}: MODEL CREATE is in the » menu at {narrow:?}") };
            let folded = rows(&mut h, panel);
            assert!(folded.iter().any(|id| id == "dropdown:solid.extrude"), "grid {grid}: Extrude, off the ribbon, is listed: {folded:?}");
        }
        let box_ = solidcraft_engine::find_command("solid.box").unwrap();
        assert_eq!(skip_on_ribbon(vec![box_], &["solid.box"]).len(), 1, "a drop-down is never left empty");
    }
}

#[cfg(test)]
mod panel_drop_tests {
    use super::*;

    #[test]
    fn panel_label_gap_and_margins_append_while_buttons_insert_before() {
        for target in ["label", "gap", "left margin", "right margin", "button"] {
            let mut h = crate::scenario::Harness::new();
            h.step(&json!({"click":{"handle":"panel:MODEL:CREATE"}})).unwrap();
            let label = crate::scenario::handle_at("panel:MODEL:CREATE").unwrap();
            let panel = crate::scenario::rect_of("panel:MODEL:CREATE").unwrap();
            let destination = match target {
                "gap" => label - vec2(0.0, 10.0),
                "left margin" => pos2(panel.left() + 1.0, label.y - 30.0),
                "right margin" => pos2(panel.right() - 1.0, label.y - 30.0),
                "button" => crate::scenario::handle_at("button:solid.extrude").unwrap(),
                _ => label,
            };
            h.step(&json!({"drag":[{"handle":"dropdown:solid.cylinder"}, [destination.x,destination.y]],"steps":12})).unwrap();
            h.step(&json!({"key":"Escape"})).unwrap();
            let order = if target == "button" {
                json!(["button:solid.cylinder", "button:solid.extrude", "button:solid.revolve", "button:solid.hole"])
            } else {
                json!(["button:solid.extrude", "button:solid.revolve", "button:solid.hole", "button:solid.cylinder"])
            };
            h.step(&json!({"expect":{"handle_order":order,"tool":null,"dialog":null,"errors":0}})).unwrap();
            assert!(h.app.session.doc.features.is_empty(), "{target}: pinning must not run the command");
            h.step(&json!({"restart":"close"})).unwrap();
            h.step(&json!({"expect":{"handle_order":order}})).unwrap();
        }
    }

    #[test]
    fn panel_label_drop_moves_a_button_between_panels() {
        let mut h = crate::scenario::Harness::new();
        h.step(&json!({"drag":[{"handle":"button:solid.extrude"},{"handle":"panel:MODEL:MODIFY"}],"steps":12})).unwrap();
        h.step(&json!({"expect":{"handle_order":["button:solid.fillet","button:solid.chamfer","button:solid.shell","button:solid.extrude"],"tool":null,"dialog":null,"errors":0}})).unwrap();
        h.step(&json!({"restart":"close"})).unwrap();
        h.step(&json!({"expect":{"handle_order":["button:solid.shell","button:solid.extrude"]}})).unwrap();
    }
}

#[cfg(test)]
mod dropdown_toggle_tests {
    use crate::scenario::{Harness, rect_of};
    use serde_json::json;

    /// A second click on the button that opened a drop-down closes it (#1522): the workspace
    /// switcher's menu, and a panel's command list.
    #[test]
    fn a_second_click_on_its_button_closes_a_drop_down() {
        let mut h = Harness::new();
        h.frames(3);
        let ws = rect_of("workspace_switcher").unwrap().center();
        h.step(&json!({"click": [ws.x, ws.y]})).unwrap();
        assert!(h.app.viewport.context_menu.is_some(), "the first click opens the workspace menu");
        h.step(&json!({"click": [ws.x, ws.y]})).unwrap();
        h.frames(2);
        assert!(h.app.viewport.context_menu.is_none(), "the second click closes it instead of reopening it");
        h.step(&json!({"click": [ws.x, ws.y]})).unwrap();
        assert!(h.app.viewport.context_menu.is_some(), "and a third opens it again");
        h.step(&json!({"key": "Escape"})).unwrap();

        h.step(&json!({"click": {"handle": "panel:MODEL:CREATE"}})).unwrap();
        h.frames(2);
        assert!(rect_of("toolbar_dropdown").is_some(), "the first click opens the panel's list");
        h.step(&json!({"click": {"handle": "panel:MODEL:CREATE"}})).unwrap();
        h.frames(2);
        assert!(rect_of("toolbar_dropdown").is_none(), "the second click closes it");
    }

    #[test]
    fn ribbon_panel_menus_follow_hover_only_after_click_and_only_when_enabled() {
        for follow_hover in [false, true] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", json!({"width":1600,"height":1000}));
            let prefs = h.call("ui.prefs", json!({"ribbon_menus_follow_hover":follow_hover}));
            assert_eq!(prefs["result"]["ribbon_menus_follow_hover"], follow_hover);
            let create = "panel:MODEL:CREATE";
            let modify = "panel:MODEL:MODIFY";
            let move_to = |h: &mut Harness, handle| {
                h.step(&json!({"move":{"handle":handle}})).unwrap();
                h.frames(3);
            };
            move_to(&mut h, create);
            assert!(rect_of("toolbar_dropdown").is_none(), "hover alone never opens a menu");
            h.step(&json!({"click":{"handle":create}})).unwrap();
            h.frames(3);
            let create_menu = rect_of("toolbar_dropdown").expect("Create menu");
            move_to(&mut h, "dropdown:solid.box");
            assert_eq!(rect_of("toolbar_dropdown"), Some(create_menu), "moving into the menu keeps it open");

            move_to(&mut h, modify);
            if follow_hover {
                let modify_menu = rect_of("toolbar_dropdown").expect("Modify menu");
                assert!(modify_menu.left() > create_menu.left(), "hover switches to the panel on the right");
                move_to(&mut h, create);
                assert_eq!(rect_of("toolbar_dropdown"), Some(create_menu), "hover also switches to the left");
            } else {
                assert_eq!(rect_of("toolbar_dropdown"), Some(create_menu), "click-only mode ignores sibling hover");
            }
            let outside = h.app.viewport.rect.unwrap().right_bottom() - egui::vec2(40.0, 60.0);
            h.step(&json!({"move":[outside.x,outside.y]})).unwrap();
            h.frames(3);
            assert_eq!(rect_of("toolbar_dropdown"), (!follow_hover).then_some(create_menu), "only hover mode closes on leaving");
            if follow_hover {
                move_to(&mut h, modify);
                assert!(rect_of("toolbar_dropdown").is_none(), "returning to the ribbon still needs a click");
            }
        }
    }

    /// A click on the lit button of the sketch tool in use ends it, so clicks in the view select
    /// again and Ctrl+click adds lines to the selection (#10151); a constraint in the ribbon's
    /// grid ends the same way. With the preference off, the click starts the tool again.
    #[test]
    fn a_second_click_on_a_running_tools_button_ends_it() {
        use solidcraft_engine::Sel;
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.set_origin_shown(false);
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[-20,-20],"p1":[20,20]})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[-10,-10],"p1":[10,10]})).unwrap();
        h.app.cam.distance = 100.0;
        h.settle();
        let grid = format!("ribbon:constraint:{}", crate::workspace::CONSTRAINT_GALLERY[0]);
        for handle in ["button:sketch.line", "button:sketch.dimension", grid.as_str()] {
            h.step(&json!({"click": {"handle": handle}})).unwrap();
            let running = h.app.tool.as_ref().map(|t| t.cmd);
            assert!(running.is_some(), "{handle}: the first click starts the tool");
            h.frames(30);
            h.step(&json!({"click": {"handle": handle}})).unwrap();
            assert!(h.app.tool.is_none(), "{handle}: the second click ends {running:?}");
            h.frames(30);
        }
        // With no tool running, clicks select, and Ctrl+click adds lines of both rectangles.
        for (i, p) in [[-15.0, 20.0], [20.0, 15.0], [-5.0, 10.0], [10.0, 5.0]].iter().enumerate() {
            let at = h.call("ui.at", json!({"sketch": p}))["result"].clone();
            h.step(&json!({"click": at, "ctrl": i > 0})).unwrap();
        }
        let curves = h.app.session.selection.iter().filter(|s| matches!(s, Sel::SketchCurve { .. })).count();
        assert_eq!(curves, 4, "{:?}", h.app.session.selection);
        // The old way, behind the preference: the click starts the tool again.
        h.app.preferences.ribbon_click_ends_tool = false;
        h.step(&json!({"click": {"handle": "button:sketch.line"}})).unwrap();
        h.frames(30);
        h.step(&json!({"click": {"handle": "button:sketch.line"}})).unwrap();
        assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some("sketch.line"));
    }

    /// A tool pinned into the Finish Sketch group, the one at the ribbon's right edge, shows
    /// pressed while it runs like any other tool button; only Finish and Cancel never do, not
    /// even under a press.
    #[test]
    fn a_tool_pinned_into_the_finish_group_shows_pressed_while_it_runs() {
        fn flat(shape: &egui::Shape, out: &mut Vec<egui::Shape>) {
            match shape {
                egui::Shape::Vec(v) => v.iter().for_each(|s| flat(s, out)),
                s => out.push(s.clone()),
            }
        }
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        crate::toolbar_custom::reorder(&mut h.app, "SKETCH", "sketch.ellipse", "FINISH SKETCH", None);
        h.settle();
        h.step(&json!({"click": {"handle": "button:sketch.ellipse"}})).unwrap();
        assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some("sketch.ellipse"), "the click starts the tool");
        h.frames(3);
        let soft = crate::workspace::tab_accent(&solidcraft_theme::Tokens::get(), "SKETCH").soft;
        let pressed = |h: &Harness, id: &str| {
            let r = rect_of(&format!("button:{id}")).unwrap();
            let mut all = Vec::new();
            h.shapes.iter().for_each(|s| flat(&s.shape, &mut all));
            all.iter().any(|s| matches!(s, egui::Shape::Rect(s) if s.rect == r && s.fill == soft))
        };
        assert!(pressed(&h, "sketch.ellipse"), "the running tool's button is pressed");
        // A press lights any other button while the pointer is down; Finish, Cancel and Finish
        // with AutoConstrain (a button only once pinned) stay plain. None runs as a tool or
        // leaves a dialog, so a press is how they would light.
        for exit in ["sketch.finish", "sketch.cancel", "sketch.finish_auto_constrain"] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            crate::toolbar_custom::pin(&mut h.app, "SKETCH", exit).unwrap();
            h.settle();
            let at = crate::scenario::handle_at(&format!("button:{exit}")).unwrap();
            h.frame_with(crate::test_support::pointer(at, true));
            assert!(!pressed(&h, exit), "a press on {exit} does not show it pressed");
        }
    }

    /// Ribbon Tabs Only folds the ribbon to its tab row (#10112) in both layouts: a tab's click
    /// opens its panels over the view, a command starting or the pointer leaving closes them, and
    /// the command again keeps the whole ribbon shown.
    #[test]
    fn ribbon_tabs_only_folds_to_the_tab_row_and_a_tab_opens_over_the_view() {
        for layout in ["compact", "floating"] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.app.run("view.layout", json!({ "mode": layout })).unwrap();
            h.frames(3);
            assert!(!h.app.preferences.ribbon_tabs_only, "off by default");
            let whole = rect_of("toolbar").unwrap();
            let whole_view = h.app.viewport.rect.unwrap();
            assert!(rect_of("ribbon:folded").is_none());

            h.app.start("view.ribbon_tabs_only");
            h.frames(3);
            assert!(h.app.preferences.ribbon_tabs_only, "{layout}: the command folds the ribbon");
            let folded = rect_of("ribbon:folded").unwrap();
            assert!(folded.height() <= super::TAB_ROW_HEIGHT + 1.0, "{layout}: only the tab row is left: {folded:?}");
            assert!(rect_of("panel:MODEL:CREATE").is_none(), "{layout}: no panels while folded");
            assert_eq!(h.app.viewport.rect.unwrap(), whole_view, "{layout}: folding preserves the canvas domain");

            // A tab's click opens its panels over the view, as tall as the whole ribbon.
            let tab = rect_of("toolbar_tab:SURFACE").unwrap().center();
            h.step(&json!({"click": [tab.x, tab.y]})).unwrap();
            h.frames(2);
            assert_eq!(h.app.ui.tab, "SURFACE");
            assert!(h.app.ui.ribbon_open, "{layout}: the tab's panels open");
            let open = rect_of("ribbon:open").unwrap();
            assert!((open.height() - whole.height()).abs() <= 2.0 * crate::ui_kit::SHELL_GAP + 1.0, "{layout}: {open:?} vs {whole:?}");
            assert!(rect_of("panel:SURFACE:CREATE").is_some(), "{layout}: the panels are drawn");
            assert_eq!(h.app.viewport.rect.unwrap(), whole_view, "{layout}: the open ribbon overlays the canvas");

            // The pointer leaving the panels closes them.
            let away = h.app.viewport.rect.unwrap().center() + egui::vec2(0.0, 150.0);
            h.step(&json!({"move": [away.x, away.y]})).unwrap();
            h.frames(2);
            assert!(!h.app.ui.ribbon_open, "{layout}: leaving closes the panels");
            assert!(rect_of("ribbon:open").is_none());

            // A command started from them closes them too.
            h.step(&json!({"click": [tab.x, tab.y]})).unwrap();
            h.frames(2);
            assert!(h.app.ui.ribbon_open);
            h.app.start("view.grid");
            h.frames(2);
            assert!(!h.app.ui.ribbon_open, "{layout}: a command closes the panels");

            // The command again keeps the whole ribbon shown.
            h.app.ui.tab = "SURFACE".into();
            h.app.start("view.ribbon_tabs_only");
            h.frames(3);
            assert!(!h.app.preferences.ribbon_tabs_only);
            assert_eq!(h.app.ui.tab, "SURFACE", "{layout}: the ribbon keeps its tab");
            assert!(rect_of("ribbon:folded").is_none());
            assert_eq!(rect_of("toolbar").unwrap().height(), whole.height(), "{layout}");
            assert!(h.app.run("view.ribbon_tabs_only", json!({"on": true})).is_ok());
            assert!(h.app.preferences.ribbon_tabs_only, "{layout}: `on` sets it");
            assert!(h.app.session.undo.is_empty(), "{layout}: a window setting, not a design change");
        }
    }

    /// Switching tabs with a panel's drop-down open forgets that drop-down, so the pointer leaving
    /// the overlay still closes it (#10112).
    #[test]
    fn ribbon_tabs_only_closes_after_a_tab_switch_with_a_panel_menu_open() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.frames(3);
        h.app.start("view.ribbon_tabs_only");
        h.frames(3);
        let tab = rect_of("toolbar_tab:MODEL").unwrap().center();
        h.step(&json!({"click": [tab.x, tab.y]})).unwrap();
        h.frames(2);
        h.step(&json!({"click": {"handle": "panel:MODEL:MODIFY"}})).unwrap();
        h.frames(2);
        let key = egui::Id::unique("sc_panel_open");
        assert!(h.app.ui.ribbon_open);
        assert!(h.ctx.data(|d| d.get_temp::<egui::Id>(key)).is_some(), "the panel menu is open");
        let other = rect_of("toolbar_tab:SURFACE").unwrap().center();
        h.step(&json!({"click": [other.x, other.y]})).unwrap();
        h.frames(2);
        assert_eq!(h.app.ui.tab, "SURFACE");
        assert!(h.app.ui.ribbon_open, "the other tab's panels are shown");
        assert!(h.ctx.data(|d| d.get_temp::<egui::Id>(key)).is_none(), "the old menu is forgotten");
        let away = h.app.viewport.rect.unwrap().center() + egui::vec2(0.0, 150.0);
        h.step(&json!({"move": [away.x, away.y]})).unwrap();
        h.frames(2);
        assert!(!h.app.ui.ribbon_open, "leaving still closes the overlay");
    }

    /// A drop-down row's help tooltip names the key the command runs from now, not its default.
    #[test]
    fn a_drop_down_row_tooltip_names_a_rebound_key() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.frames(3);
        crate::keymap::bind(&mut h.app, "solid.shell", "Shift+J", false).unwrap();
        h.step(&json!({"click": {"handle": "panel:MODEL:MODIFY"}})).unwrap();
        h.frames(2);
        h.step(&json!({"move": {"handle": "dropdown:solid.shell"}})).unwrap();
        h.frames(40);
        let shell = h.app.tr_label("Shell").to_string();
        assert!(h.widget(&format!("{shell}  (Shift+J)")).is_some(), "the tooltip shows the new key");
    }
}

#[cfg(test)]
mod app_bar_tests {
    use super::*;

    #[test]
    fn bar_hover_inks_and_dividers_remain_readable_in_both_themes() {
        fn luminance(c: Color32) -> f32 {
            let c = egui::Rgba::from(c);
            0.2126 * c.r() + 0.7152 * c.g() + 0.0722 * c.b()
        }
        fn contrast(a: Color32, b: Color32) -> f32 {
            let (a, b) = (luminance(a), luminance(b));
            (a.max(b) + 0.05) / (a.min(b) + 0.05)
        }
        for dark in [false, true] {
            for vision in solidcraft_theme::ColorVision::ALL {
                let t = Tokens::themed(dark, vision);
                for fill in [t.app_bar_hover, t.app_bar_hover_subtle, t.app_bar_hover_button, t.app_bar_hover_strong] {
                    let channel = |fg: u8, bg: u8| fg.saturating_add((u16::from(bg) * u16::from(255 - fill.a()) / 255) as u8);
                    let bg = Color32::from_rgb(channel(fill.r(), t.app_bar.r()), channel(fill.g(), t.app_bar.g()), channel(fill.b(), t.app_bar.b()));
                    assert!(contrast(t.app_bar_text, bg) >= 4.5, "dark={dark}, {vision:?}, {fill:?}");
                }
                assert!(contrast(t.app_bar_separator, t.app_bar) >= 3.0);
                assert!(contrast(t.app_bar, t.warning) >= 4.5, "approval counts need dark ink on amber");
            }
        }
    }

    /// Save writes on a worker thread: frames until it has finished.
    fn wait_for_save(h: &mut crate::scenario::Harness) {
        for _ in 0..500 {
            if h.app.saving.is_none() {
                break;
            }
            h.frames(1);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        h.frames(1);
    }

    #[test]
    fn save_button_uses_the_existing_save_flow_for_new_and_named_designs() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length":2,"width":3,"height":4})).unwrap();
        h.frames(3);
        h.step(&json!({"click":{"handle":"titlebar:save"}})).unwrap();
        wait_for_save(&mut h);
        let path = h.app.session.path.clone().expect("Save chooses a path for an untitled design");
        h.app.run("solid.box", json!({"length":1,"width":1,"height":1,"corner":[10,0,0]})).unwrap();
        h.step(&json!({"click":{"handle":"titlebar:save"}})).unwrap();
        wait_for_save(&mut h);
        assert_eq!(h.app.session.path.as_deref(), Some(path.as_str()));
        let saved = solidcraft_engine::io::read_design(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved, *h.app.session.doc);
    }
}

#[cfg(test)]
mod ribbon_label_tests {
    use super::*;

    #[test]
    fn translated_workspace_captions_fit_with_room_for_the_arrow() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        for layout in ["floating", "compact"] {
            h.call("ui.set", json!({"layout": layout}));
            for labels in [true, false] {
                h.app.preferences.ribbon_labels = labels;
                for language in ["en", "ru", "de", "ja", "ko", "uk"] {
                    h.call("ui.prefs", json!({"language": language}));
                    h.frames(3);
                    let ws = crate::scenario::rect_of("workspace_switcher").unwrap();
                    let caption = h.app.tr("ui.ribbon_workspace");
                    let text = h
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) if text.galley.text() == caption => Some(text),
                            _ => None,
                        })
                        .unwrap();
                    let bounds = Rect::from_min_size(text.pos, text.galley.size());
                    assert!(bounds.left() >= ws.left() + 4.0, "{language}, {layout}, labels={labels}: {bounds:?} outside {ws:?}");
                    assert!(bounds.right() + 12.0 <= ws.right() - 4.0, "{language}, {layout}, labels={labels}: no room for the arrow");
                    if language == "en" {
                        assert!(!text.galley.elided, "the English caption still fits whole");
                    }
                }
            }
        }
    }

    fn tab_test_themes() -> Vec<Tokens> {
        crate::test_support::host_data();
        let mut themes = Vec::new();
        for dark in [false, true] {
            for vision in solidcraft_theme::ColorVision::ALL {
                themes.push(Tokens::themed(dark, vision));
            }
        }
        themes.extend(crate::theme_files::builtin().iter().map(crate::theme_files::resolve));
        themes
    }

    #[test]
    fn a_selected_contextual_tab_stands_out_and_an_unselected_one_is_outlined() {
        for t in tab_test_themes() {
            for tab in ["SKETCH", "SHEET METAL"] {
                let accent = crate::workspace::tab_accent(&t, tab);
                for hovered in [false, true] {
                    let active = ribbon::tab_style(&t, crate::workspace::tab_accent(&t, tab), true, true, hovered);
                    let inactive = ribbon::tab_style(&t, crate::workspace::tab_accent(&t, tab), true, false, hovered);
                    assert_eq!(active, ribbon::TabStyle { fill: accent.color, ink: accent.on_fill(&t), outline: Color32::TRANSPARENT });
                    assert_eq!(
                        inactive,
                        ribbon::TabStyle {
                            outline: accent.color,
                            ..ribbon::tab_style(&t, crate::workspace::tab_accent(&t, tab), false, false, hovered)
                        }
                    );
                    assert!(solidcraft_theme::contrast_ratio(active.ink, active.fill) >= 4.5, "{tab}: selected text must remain readable");
                    // The selected contextual tab is the solid pill; the unselected one never is.
                    assert!(
                        solidcraft_theme::contrast_ratio(active.fill, t.toolbar)
                            > solidcraft_theme::contrast_ratio(if hovered { t.hover } else { t.toolbar }, t.toolbar),
                        "{tab}: the selected tab must be the more prominent one"
                    );
                }
            }
        }
    }

    #[test]
    fn ordinary_tabs_keep_their_selected_hovered_and_idle_colors() {
        for t in tab_test_themes() {
            for tab in ["MODEL", "SURFACE", "SHEET METAL"] {
                let accent = crate::workspace::tab_accent(&t, tab);
                for hovered in [false, true] {
                    let none = Color32::TRANSPARENT;
                    assert_eq!(
                        ribbon::tab_style(&t, crate::workspace::tab_accent(&t, tab), false, true, hovered),
                        ribbon::TabStyle { fill: accent.soft, ink: accent.ink, outline: none }
                    );
                    let idle = if hovered { t.hover } else { none };
                    assert_eq!(
                        ribbon::tab_style(&t, crate::workspace::tab_accent(&t, tab), false, false, hovered),
                        ribbon::TabStyle { fill: idle, ink: t.text, outline: none }
                    );
                }
            }
        }
    }

    #[test]
    fn ribbon_weight_is_painted_once_at_a_pixel_aligned_origin() {
        for scale in [1.0, 1.5, 2.0] {
            let ctx = crate::fonts::test_context();
            ctx.set_pixels_per_point(scale);
            let ink = Tokens::get().text_dim;
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                ribbon::text(ui, pos2(100.3, 40.7), "Selected tab", 13.0, ink, true);
            });
            output.textures_delta.clear();
            let text: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(text) if text.galley.text() == "Selected tab" => Some(text),
                    _ => None,
                })
                .collect();
            assert_eq!(text.len(), 1, "ribbon labels must never be drawn twice");
            let text = text[0];
            assert_eq!(text.galley.job.sections[0].format.font_id, crate::ui_kit::type_face::semibold(13.0));
            assert_eq!(text.galley.job.sections[0].format.color, ink);
            assert!((text.pos.x * scale - (text.pos.x * scale).round()).abs() < 0.001);
            assert!((text.pos.y * scale - (text.pos.y * scale).round()).abs() < 0.001);
        }
    }

    /// Every promoted button's label, in every language, sits between the button's icon and its
    /// bottom edge, so the hover highlight holds all of it (#8391). One line stays where it was.
    #[test]
    fn every_promoted_label_fits_in_its_button_below_the_icon() {
        let ctx = crate::fonts::test_context();
        let mut two_lines = 0;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.painter().clone();
            let specs = solidcraft_engine::command_specs();
            for section in crate::workspace::SECTIONS {
                let (commands, count) = crate::workspace::layout(section.tab, section.panel, &specs).unwrap();
                for command in commands.iter().take(count) {
                    for catalog in crate::i18n::LOCALES {
                        let locale = solidcraft_ui_text::Locale::for_language(catalog.id);
                        let label = locale.label(crate::workspace::ribbon_button_label(command));
                        let whole = Rect::from_min_size(pos2(40.0, 2.0), vec2(ribbon::button_width(&painter, label), ribbon::BUTTON_HEIGHT));
                        let galley = ribbon::button_galley(&painter, label, whole.width() - ribbon::BUTTON_LABEL_PADDING, Color32::WHITE);
                        let top = ribbon::label_top(whole, galley.size().y);
                        let at = format!("{} in {}: {label:?}", command.id, catalog.id);
                        // The icon is 26 points square, centred 16 points below the button's top.
                        assert!(top >= whole.top() + 16.0 + 13.0, "{at} starts at {top} over the icon");
                        assert!(top + galley.size().y <= whole.bottom() - ribbon::BUTTON_LABEL_BOTTOM + 0.01, "{at} runs past the button");
                        if galley.rows.len() == 1 {
                            assert_eq!(top, whole.top() + ribbon::BUTTON_LABEL_TOP, "{at}");
                        } else {
                            two_lines += 1;
                        }
                    }
                }
            }
        });
        output.textures_delta.clear();
        assert!(two_lines > 0, "some labels take two lines");
    }

    #[test]
    fn every_promoted_label_wraps_only_at_word_boundaries() {
        let ctx = crate::fonts::test_context();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.painter().clone();
            let specs = solidcraft_engine::command_specs();
            for section in crate::workspace::SECTIONS {
                let (commands, count) = crate::workspace::layout(section.tab, section.panel, &specs).unwrap();
                for command in commands.iter().take(count) {
                    for catalog in crate::i18n::LOCALES {
                        let locale = solidcraft_ui_text::Locale::for_language(catalog.id);
                        let label = locale.label(crate::workspace::ribbon_button_label(command));
                        let width = ribbon::button_width(&painter, label) - ribbon::BUTTON_LABEL_PADDING;
                        let galley = ribbon::button_galley(&painter, label, width, Color32::WHITE);
                        assert!(galley.rows.len() <= 2, "{}: {label}", command.id);
                        assert!(galley.size().x <= width + 0.01, "{}: {label}: {} > {width}", command.id, galley.size().x);
                        if solidcraft_ui_text::bidi::has_rtl(label) {
                            // epaint adds zero-width glyphs to right-to-left rows, so their text
                            // is not the source text: check the lines the galley is built from.
                            let lines = ribbon::button_lines(&painter, label, width);
                            let first = lines[0].strip_prefix('…').unwrap_or(&lines[0]);
                            assert!(label.ends_with(first), "{}: {label} -> {lines:?}", command.id);
                            if let [first, second] = lines.as_slice()
                                && !first.starts_with('…')
                                && !second.starts_with('…')
                            {
                                // Both lines whole: they split the label at a space or after a slash.
                                assert!(
                                    *label == format!("{second} {first}") || *label == format!("{second}{first}"),
                                    "{}: {label} -> {lines:?}",
                                    command.id
                                );
                            }
                            continue;
                        }
                        let first = galley.rows.first().unwrap().text();
                        let visible = first.strip_suffix('…').unwrap_or(&first);
                        assert!(label.starts_with(visible), "{}: {label} -> {first}", command.id);
                        if let Some(second) = galley.rows.get(1) {
                            let second = second.text();
                            let next = second.strip_suffix('…').unwrap_or(&second);
                            // The second line must start at a legal source boundary,
                            // including when the first line was ellipsized.
                            assert!(
                                label.char_indices().any(|(at, ch)| {
                                    let end = at + ch.len_utf8();
                                    (ch.is_whitespace() || ch == '/' || ribbon::cjk_word_break(label, end))
                                        && label[end..].trim_start().starts_with(next)
                                        && (first.ends_with('…') || label[..end].trim() == first)
                                }),
                                "{}: {label} -> {first:?}, {second:?} breaks inside a word",
                                command.id
                            );
                        }
                    }
                }
            }
            for label in ["Rectangular", "Dimension", "Coincident", "Chamfer", "Revolve"] {
                let width = ribbon::button_width(&painter, label);
                assert_eq!(ribbon::button_lines(&painter, label, width - ribbon::BUTTON_LABEL_PADDING), vec![label]);
                assert!((BUTTON_MIN_WIDTH..=ribbon::BUTTON_MAX_WIDTH).contains(&width));
            }
            let finish = solidcraft_engine::find_command("sketch.finish").unwrap();
            let label = crate::workspace::ribbon_button_label(finish);
            let width = ribbon::button_width(&painter, label);
            assert_eq!(ribbon::button_lines(&painter, label, width - ribbon::BUTTON_LABEL_PADDING), vec!["Finish", "Sketch"]);
            // Exercise full labels and pinned commands too, not just compact defaults.
            for (label, width, expected) in [
                ("Coincident", 50.0, vec!["Coincident"]),
                ("Rectangular Pattern", 50.0, vec!["Rectangular", "Pattern"]),
                ("Move/Copy", 40.0, vec!["Move/", "Copy"]),
            ] {
                let lines = ribbon::button_lines(&painter, label, width);
                // Exact glyph widths depend on the bundled font: verify the source
                // word stays on its own line, with only an optional suffix omitted.
                assert_eq!(lines.len(), expected.len(), "{label}: {lines:?}");
                for (actual, word) in lines.iter().zip(expected) {
                    assert!(actual == word || actual.strip_suffix('…').is_some_and(|prefix| word.starts_with(prefix)), "{label}: {lines:?}");
                }
            }
            let lines = ribbon::button_lines(&painter, "one two three four five", 50.0);
            assert_eq!(lines.len(), 2);
            assert!(lines[1].ends_with('…'));
            let lines = ribbon::button_lines(&painter, "長い日本語のラベル 続き", 50.0);
            assert_eq!(lines.len(), 2);
            assert!(lines[0].ends_with('…'));
            // Hebrew comes in display order (`solidcraft_ui_text::bidi::visual`): the reader starts at the right end,
            // so "סקיצה חדשה" (New Sketch, stored as "חדשה סקיצה") puts סקיצה on the first line.
            let label = solidcraft_ui_text::bidi::visual("סקיצה חדשה").into_owned();
            let width = ribbon::label_width(&painter, "סקיצה").max(ribbon::label_width(&painter, "חדשה")) + 1.0;
            assert!(ribbon::label_width(&painter, &label) > width);
            assert_eq!(ribbon::button_lines(&painter, &label, width), vec!["סקיצה", "חדשה"]);
            let lines = ribbon::button_lines(&painter, &solidcraft_ui_text::bidi::visual("אחת שתיים שלוש ארבע חמש"), 50.0);
            assert_eq!(lines.len(), 2);
            assert!(lines[1].starts_with('…'), "{lines:?}");
        });
        output.textures_delta.clear();
        // Japanese is measured with the bundled Japanese face, as a Japanese UI draws it: without it
        // the glyphs are missing and their width says nothing.
        let ctx = crate::fonts::test_context();
        let cjk = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/biz-udp-gothic/BIZUDPGothic-Regular.ttf"));
        assert!(crate::fonts::install_cjk_font_for(&ctx, cjk.to_vec(), "ja"));
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.painter().clone();
            let ja = solidcraft_ui_text::Locale::for_language("ja");
            for (id, expected) in [
                ("sketch.create", vec!["新規", "スケッチ"]),
                ("config.table", vec!["構成", "テーブル"]),
                ("construct.plane.offset", vec!["オフセット", "平面"]),
                ("surface.loft", vec!["サーフェス", "ロフト"]),
                ("mesh.convert", vec!["メッシュを", "変換"]),
                ("component.create", vec!["新規", "コンポ"]),
                ("joint.origin", vec!["ジョイント", "原点"]),
                ("joint.drive", vec!["ジョイントを", "駆動"]),
            ] {
                let command = solidcraft_engine::find_command(id).unwrap();
                let label = ja.label(crate::workspace::ribbon_button_label(command));
                let width = ribbon::button_width(&painter, label) - ribbon::BUTTON_LABEL_PADDING;
                assert_eq!(ribbon::button_lines(&painter, label, width), expected, "{id}");
            }
            let comp = solidcraft_engine::find_command("component.create").unwrap();
            assert_eq!(ja.label(comp.label), "新規コンポーネント");
        });
        output.textures_delta.clear();
    }
}

#[cfg(test)]
mod ribbon_gallery_tests {
    use super::*;
    use crate::scenario::{Harness, rect_of};

    #[test]
    fn dragged_constraint_becomes_full_button_and_reset_restores_gallery() {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1800,"height":900}));
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.frames(3);
        let id = crate::workspace::CONSTRAINT_GALLERY[0];
        let cell_handle = format!("ribbon:constraint:{id}");
        let button_handle = format!("toolbar:{id}");
        let button = format!("button:{id}");
        let cell = rect_of(&cell_handle).unwrap();
        let count = || crate::scenario::handles("ribbon:constraint:").len();
        let original_count = count();
        h.step(&json!({"drag":[{"handle":button_handle},{"handle":"toolbar:sketch.dimension"}]})).unwrap();
        h.frames(3);
        assert!(rect_of(&cell_handle).is_none());
        let full = rect_of(&button).unwrap();
        assert!(full.height() > cell.height());
        assert!(full.left() < rect_of("button:sketch.dimension").unwrap().left());
        assert_eq!(count(), original_count - 1);
        let next = format!("ribbon:constraint:{}", crate::workspace::CONSTRAINT_GALLERY[1]);
        assert_eq!(rect_of(&next).unwrap().top(), cell.top());
        h.step(&json!({"drag":[{"handle":button_handle},{"handle":next}]})).unwrap();
        h.frames(3);
        assert_eq!(rect_of(&cell_handle), Some(cell));
        assert_eq!(count(), original_count);
        h.step(&json!({"drag":[{"handle":button_handle},{"handle":"toolbar:sketch.dimension"}]})).unwrap();
        h.frames(3);
        assert!(rect_of(&cell_handle).is_none());
        let saved = crate::toolbar_custom::export(&h.app);
        crate::toolbar_custom::reset(&mut h.app, "SKETCH");
        crate::toolbar_custom::import(&mut h.app, &saved).unwrap();
        h.frames(3);
        assert!(rect_of(&cell_handle).is_none());
        assert_eq!(rect_of(&button).unwrap().height(), full.height());
        // Settings saved before the gallery could change load with the default gallery.
        let mut older = h.app.toolbar_custom.prefs();
        older.as_object_mut().unwrap().remove("gallery_buttons");
        let mut loaded = crate::toolbar_custom::ToolbarCustom::default();
        loaded.load(&older);
        assert_eq!(loaded.panels, h.app.toolbar_custom.panels);
        assert!(loaded.in_gallery("SKETCH", "CONSTRAIN", id));
        crate::toolbar_custom::reset(&mut h.app, "MODEL");
        h.frames(2);
        assert!(rect_of(&cell_handle).is_none());
        crate::toolbar_custom::reset(&mut h.app, "SKETCH");
        h.frames(3);
        assert_eq!(rect_of(&cell_handle), Some(cell));
        assert_eq!(count(), original_count);
    }

    #[test]
    fn constraint_gallery_preserves_button_handles_order_and_pin_actions() {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1800,"height":900}));
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.frames(3);
        let dimension = rect_of("button:sketch.dimension").unwrap();
        let mut cells = Vec::new();
        for id in crate::workspace::CONSTRAINT_GALLERY {
            let cell = rect_of(&format!("ribbon:constraint:{id}")).unwrap();
            assert_eq!(cell.size(), vec2(24.0, 20.0));
            assert_eq!(rect_of(&format!("button:{id}")), Some(cell));
            assert!(cell.left() > dimension.right());
            cells.push(cell);
        }
        for row in cells.chunks(3) {
            assert!(row.windows(2).all(|pair| pair[0].right() < pair[1].left() && pair[0].top() == pair[1].top()));
        }
        for col in 0..3 {
            assert_eq!(cells[col].left(), cells[col + 3].left());
            assert!(cells[col].bottom() < cells[col + 3].top());
        }
        h.step(&json!({"click":{"handle":"ribbon:constraint:sketch.constraint.equal"}})).unwrap();
        assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some("sketch.constraint.equal"));
        h.step(&json!({"key":"Escape"})).unwrap();
        crate::toolbar_custom::remove(&mut h.app, "SKETCH", "sketch.constraint.equal");
        h.frames(2);
        assert!(rect_of("ribbon:constraint:sketch.constraint.equal").is_none());
        crate::toolbar_custom::pin(&mut h.app, "SKETCH", "sketch.constraint.equal").unwrap();
        h.frames(2);
        assert!(rect_of("ribbon:constraint:sketch.constraint.equal").is_some());
    }
}

#[cfg(test)]
mod fill_width_tests {
    use super::*;

    /// #2108: at every common window width, in the Solid and the Sketch tab, the toolbar uses the
    /// width it has. No blank run wider than one button while something is hidden, every panel
    /// on the toolbar shows its icons, the » menu only when even one button a panel won't fit,
    /// and neither the tab row nor the title bar's own controls run into each other.
    #[test]
    fn toolbar_fills_its_width_from_wide_to_narrow_windows() {
        // Either header layout: the ribbon fits the same way with search in the bar.
        for (sketch, layout) in [false, true].into_iter().flat_map(|s| [(s, HeaderLayout::DESKTOP), (s, HeaderLayout::COMPACT)]) {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.app.custom_titlebar = true;
            h.app.header = layout;
            h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
            if sketch {
                h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            }
            let tab = if sketch { "SKETCH" } else { "MODEL" };
            h.app.ui.tab = tab.into();
            for w in [1920.0, 1600.0, 1440.0, 1366.0, 1280.0, 1100.0, 900.0, 700.0] {
                h.call("ui.resize", json!({"width":w,"height":900}));
                h.frames(3);
                let at = format!("{tab} at {w}, {layout:?}");
                let bar = crate::scenario::rect_of("toolbar").unwrap();
                let finish = crate::scenario::rect_of("panel:SKETCH:FINISH SKETCH");
                // The room for the other panels: the bar less the Finish group kept for it.
                let right = bar.right() - 4.0 - finish.map_or(0.0, |f| f.width() + 4.0);
                let count = |name: &str| crate::scenario::count_of(name).unwrap_or(-1.0);
                let (collapsed, trimmed, more) = (count("toolbar_collapsed"), count("toolbar_trimmed"), count("toolbar_more"));
                // FINISH EDITING is shown only while a linked design is edited in place.
                let lists: Vec<_> = crate::toolbar_custom::panel_lists(tab).into_iter().filter(|(panel, _, _)| *panel != "FINISH EDITING").collect();
                let mut end = bar.left();
                let mut on_bar = 0;
                for (panel, cmds, promote) in &lists {
                    let Some(rect) = crate::scenario::rect_of(&format!("panel:{tab}:{panel}")) else { continue };
                    on_bar += 1;
                    if *panel != "FINISH SKETCH" {
                        end = end.max(rect.right() + ribbon::PANEL_GAP);
                    }
                    if crate::toolbar_custom::buttons(&h.app, tab, panel, cmds, *promote).is_empty() {
                        continue;
                    }
                    let icons = crate::scenario::handles("button:").iter().filter(|(_, p)| rect.contains(*p)).count()
                        + usize::from(crate::scenario::handle_at(&format!("collapsed:{tab}:{panel}")).is_some());
                    assert!(icons >= 1, "{at}: {panel} shows no icons");
                }
                assert_eq!(on_bar + more as usize, lists.len(), "{at}: every panel is on the toolbar or in the » menu");
                let overflow = crate::scenario::rect_of("toolbar:overflow");
                assert_eq!(overflow.is_some(), more > 0.0, "{at}: the » menu shows exactly when a panel is in it");
                let gap = right - end - if more > 0.0 { ribbon::MORE_ROOM } else { 0.0 };
                assert!(gap >= -0.5, "{at}: the panels run {} px past the room", -gap);
                let hidden = collapsed + trimmed + more > 0.0;
                let button = if more > 0.0 {
                    ribbon::COLLAPSED_WIDTH + ribbon::PANEL_GAP
                } else if sketch {
                    ribbon::GALLERY_WIDTH + ribbon::BUTTON_GAP
                } else {
                    BUTTON_MIN_WIDTH + ribbon::BUTTON_GAP
                };
                if hidden {
                    assert!(gap < button, "{at}: {gap} px left blank while buttons are hidden");
                }
                // The » menu only when one button a panel can't fit.
                if w >= 900.0 {
                    assert_eq!(more, 0.0, "{at}: the » menu while there is room");
                }
                if w >= 1600.0 {
                    assert!(!hidden, "{at}: buttons hidden on a wide window");
                }
                if let Some(o) = overflow {
                    assert!(o.right() <= right + 4.0, "{at}: the » button overlaps Finish or the right edge");
                }
                if sketch {
                    let finish = finish.expect("Finish group stays visible");
                    if more > 0.0 {
                        assert_eq!(finish.right(), bar.right(), "{at}: Finish moves right of the » menu");
                    } else {
                        assert!((finish.left() - end).abs() < 0.5, "{at}: Finish follows the last panel ({} vs {end})", finish.left());
                    }
                    assert!(crate::scenario::handle_at("button:sketch.finish").is_some(), "{at}: Finish stays reachable");
                }
                // The assistant lives in the title bar; visible ribbon tabs fit before search (or,
                // with search in the title bar, within the ribbon card).
                let search = crate::scenario::rect_of("ribbon:search").unwrap();
                let card = crate::scenario::rect_of("ribbon:card").unwrap_or(bar);
                for t in TABS.iter().chain(["SKETCH"].iter()) {
                    if let Some(r) = crate::scenario::rect_of(&format!("toolbar_tab:{t}")) {
                        assert!(r.right() < search.left() || search.bottom() <= APP_BAR_HEIGHT, "{at}: the {t} tab runs into search");
                        assert!(r.right() <= card.right(), "{at}: the {t} tab runs past the ribbon");
                    }
                }
                // The title bar keeps document tabs before Assistant, Preferences, Help and Report.
                let report = crate::scenario::rect_of("titlebar:report").unwrap();
                let tabs_end = crate::scenario::handles("tab:design:").iter().map(|(_, p)| p.x).fold(0.0, f32::max);
                assert!(tabs_end < report.left(), "{at}: the document tabs run into Report a bug");
                let assistant = crate::scenario::rect_of("titlebar:assistant").unwrap();
                let help = crate::scenario::rect_of("appbar:help").unwrap();
                let prefs = crate::scenario::rect_of("titlebar:preferences").unwrap();
                assert!(tabs_end < assistant.left(), "{at}: the document tabs run into Assistant");
                assert!(
                    assistant.right() <= prefs.left() && prefs.right() <= help.left() && help.right() <= report.left(),
                    "{at}: the title bar's right-hand controls overlap"
                );
                if layout.search == SearchPlace::Header {
                    assert!(assistant.right() <= search.left() && search.right() <= prefs.left(), "{at}: search sits between Assistant and Settings");
                    assert!(tabs_end < search.left(), "{at}: the document tabs run into search");
                }
            }
        }
    }

    /// #7166: a panel whose buttons were all removed keeps one button, its first command's icon,
    /// which opens the panel's drop-down: New Sketch stays reachable from the Model tab instead of
    /// leaving an empty Sketch panel. It takes the room an empty panel took before.
    #[test]
    fn a_panel_without_buttons_keeps_one_that_opens_its_menu() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1600,"height":900}));
        crate::toolbar_custom::remove(&mut h.app, "MODEL", "sketch.create");
        assert!(crate::toolbar_custom::pinned_on(&h.app, "MODEL", "sketch.create").is_none());
        h.frames(3);
        assert!(crate::scenario::handle_at("button:sketch.create").is_none(), "New Sketch is no longer a full button");
        let panel = crate::scenario::rect_of("panel:MODEL:SKETCH").unwrap();
        assert_eq!(panel.width(), ribbon::COLLAPSED_WIDTH, "the panel takes a collapsed panel's room");
        let button = crate::scenario::handle_at("collapsed:MODEL:SKETCH").expect("the Sketch panel keeps a button");
        assert!(panel.contains(button));
        h.step(&json!({"click":[button.x, button.y]})).unwrap();
        h.frames(2);
        assert!(crate::scenario::rect_of("toolbar_dropdown").is_some(), "the button opens the panel's drop-down");
        h.step(&json!({"click":{"handle":"dropdown:sketch.create"}})).unwrap();
        assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::Sketch)), "New Sketch starts from it");
    }
}

#[cfg(test)]
mod sketch_return_tab_tests {
    use super::*;

    /// #3250: finishing a sketch goes back to the tab it was started from, not always to MODEL.
    #[test]
    fn finishing_a_sketch_returns_to_the_tab_it_started_from() {
        for start in ["MODEL", "SURFACE", "ASSEMBLE", "INSERT", "INSPECT", "MANAGE", "VIEW"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.app.ui.tab = start.into();
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            assert_eq!(h.app.ui.tab, "SKETCH", "{start}: the sketch tab shows while sketching");
            h.app.run("sketch.finish", json!({})).unwrap();
            assert!(h.app.session.active_sketch.is_none());
            assert_eq!(h.app.ui.tab, start, "{start}: back on the tab the sketch started from");
        }
    }

    /// A second sketch started from another tab returns to that tab, and a start tab that is
    /// not a normal toolbar tab (a stale name) falls back to MODEL.
    #[test]
    fn sketch_return_tab_follows_each_sketch_and_falls_back_to_model() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        for start in ["SURFACE", "INSPECT"] {
            h.app.ui.tab = start.into();
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            h.app.run("sketch.finish", json!({})).unwrap();
            assert_eq!(h.app.ui.tab, start);
        }
        let start = "NO SUCH TAB";
        h.app.ui.tab = start.into();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        assert_eq!(h.app.ui.tab, "MODEL", "{start}: falls back to MODEL");
    }
}

#[cfg(test)]
mod command_start_tab_tests {
    use super::*;
    use crate::scenario::Harness;

    /// The issue's starting point: a design with a box, nothing selected, the ribbon on MODEL.
    fn on_model_with_a_box() -> Harness {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
        h.app.ui.tab = "MODEL".into();
        h.app.status = None;
        h
    }

    /// #10540: a command that only reports a missing parameter or that there is nothing to work
    /// on starts nothing, so the ribbon stays on the tab it was started from, as a disabled
    /// command already does. The commands are the issue's table, started from MODEL.
    #[test]
    fn a_refused_command_leaves_the_ribbon_on_the_tab_it_was_started_from() {
        for id in [
            "occurrence.ground",
            "joint.limits",
            "component.break_link",
            "timeline.dependents",
            "view.rename",
            "view.delete",
            "view.navigate",
            "edit.restore_checkpoint",
            "parameters.move",
            "library.inspect",
            // surface.fair and surface.match open their dialogs (#4644), so they start something.
            "sheet.export_bend_table",
        ] {
            let mut h = on_model_with_a_box();
            h.app.start(id);
            assert_eq!(h.app.ui.tab, "MODEL", "{id}: a refused command leaves the tab where it was");
            assert!(h.app.dialog.is_none() && h.app.tool.is_none(), "{id}: nothing started");
        }
    }

    /// The issue's exact repro through the control channel: Ground from MODEL says `occurrence`
    /// must be an id or name; the status line keeps the refusal, the ribbon does not move to
    /// ASSEMBLE.
    #[test]
    fn ground_from_command_search_names_the_missing_occurrence_and_stays_on_model() {
        let mut h = on_model_with_a_box();
        h.call("ui.start", json!({"command": "occurrence.ground"}));
        assert_eq!(h.app.ui.tab, "MODEL", "Ground started nothing: the ribbon stays on MODEL");
        let (status, _, error) = h.app.status.clone().expect("the refusal reaches the status line");
        assert!(error, "{status}");
        assert!(status.contains("occurrence"), "{status}");
    }

    /// #10540: View Fit, Home and the other view display commands act on the view the same way
    /// however they are started. F6, the navigation bar and the view cube never move the ribbon,
    /// so command search and the VIEW tab's menus must not either.
    #[test]
    fn view_display_commands_keep_the_ribbon_on_the_tab_they_were_started_from() {
        for id in ["view.fit", "view.home", "view.spin", "view.grid", "view.ground_shadow", "view.navigation_bar", "view.visual_style", "view.tool"] {
            let mut h = on_model_with_a_box();
            h.app.ui.tab = "INSPECT".into();
            h.app.start(id);
            assert_eq!(h.app.ui.tab, "INSPECT", "{id}: started from INSPECT, the ribbon stays there");
        }
    }

    /// The other half of #10540: a command that does start still moves the ribbon to its tab —
    /// Extrude's dialog opens and the ribbon follows it to MODEL from INSPECT.
    #[test]
    fn a_command_that_opens_still_moves_the_ribbon_to_its_tab() {
        let mut h = on_model_with_a_box();
        h.app.ui.tab = "INSPECT".into();
        h.app.start("solid.extrude");
        assert!(h.app.dialog.is_some(), "Extrude's dialog opens");
        assert_eq!(h.app.ui.tab, "MODEL", "the ribbon follows the command that started");
    }

    /// A command that runs without a dialog or tool has run: the ribbon keeps the tab it moved
    /// to (#10540's "or it runs"). List Contact Sets answers with the sets, no parameters.
    #[test]
    fn a_command_that_runs_still_moves_the_ribbon() {
        let mut h = on_model_with_a_box();
        h.app.start("contact.list");
        assert_eq!(h.app.ui.tab, "ASSEMBLE", "the command ran: the ribbon stays on its tab");
        assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
    }
}

#[cfg(test)]
mod persistent_ribbon_tests {
    use super::*;
    use crate::scenario::{self, Harness};

    fn harness() -> Harness {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":2400,"height":1000}));
        h.app.session.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
        h.app.ui.tab = "INSPECT".into();
        for id in PERSISTENT_COMMANDS {
            crate::toolbar_custom::pin(&mut h.app, "INSPECT", id).unwrap();
        }
        h.frames(3);
        h
    }

    fn shapes(shape: &egui::Shape, out: &mut Vec<egui::Shape>) {
        match shape {
            egui::Shape::Vec(v) => {
                for s in v {
                    shapes(s, out);
                }
            }
            s => out.push(s.clone()),
        }
    }

    fn button_shapes(h: &Harness, id: &str) -> Vec<egui::Shape> {
        let r = scenario::rect_of(&format!("button:{id}")).unwrap();
        let mut all = Vec::new();
        for s in &h.shapes {
            shapes(&s.shape, &mut all);
        }
        all.into_iter().filter(|s| r.intersects(s.visual_bounding_rect())).collect()
    }

    fn assert_paint(h: &Harness, id: &str, selected: bool) {
        assert_presentation(h, id, selected, true);
    }

    fn assert_presentation(h: &Harness, id: &str, selected: bool, check_fill: bool) {
        let r = scenario::rect_of(&format!("button:{id}")).unwrap();
        let t = Tokens::get();
        let shapes = button_shapes(h, id);
        if check_fill {
            assert_eq!(
                shapes.iter().any(|s| matches!(s, egui::Shape::Rect(s) if s.rect == r && s.fill == t.selected_bg)),
                selected,
                "{id} selected fill"
            );
        }
        assert_eq!(shapes.iter().any(|s| matches!(s, egui::Shape::LineSegment { points, stroke } if stroke.width == 2.0 && stroke.color == t.accent && points[0].y == r.bottom() - t.space_4)), selected, "{id} underline");
        if h.app.preferences.ribbon_labels {
            let text = shapes
                .iter()
                .find_map(|s| match s {
                    egui::Shape::Text(s) if r.contains(s.pos) => Some(s),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("missing {id} label"));
            let font = &text.galley.job.sections[0].format.font_id;
            assert_eq!(*font, if selected { crate::ui_kit::type_face::semibold(t.font_body) } else { FontId::proportional(t.font_body) });
            assert_eq!(text.galley.job.sections[0].format.color, if selected { t.selected_ink } else { t.text });
        }
    }

    fn tap(h: &mut Harness, id: &str) {
        let at = scenario::handle_at(&format!("button:{id}")).unwrap();
        let before = h.app.session.status_items();
        h.frame_with(crate::test_support::pointer(at, true));
        assert_eq!(h.app.session.status_items(), before, "pointer-down is not a persistent activation");
        for command in PERSISTENT_COMMANDS {
            // Existing pointer-down paint is transient and may use the same palette fill.
            // The underline, actual weight and published identities distinguish persistence.
            assert_presentation(h, command, before.iter().any(|item| item.command == *command), *command != id);
            assert_eq!(scenario::handle_at(&format!("ribbon:pressed:{command}")).is_some(), before.iter().any(|item| item.command == *command));
        }
        h.frame_with(crate::test_support::pointer(at, false));
    }

    #[test]
    fn persistent_pressed_and_search_pulse_keep_independent_effect_and_paint_authority() {
        let mut h = harness();
        for enabled in [true, false] {
            h.call("ui.prefs", json!({"persistent_ribbon_pressed":enabled,"ribbon_reveal":true}));
            h.app.sbox.reveal = Some(crate::shortcut_box::Reveal {
                id: "inspect.zebra",
                since: h.ctx.input(|input| input.time) - crate::shortcut_box::REVEAL_PULSE / 4.0,
                preview: false,
                back: "INSPECT".into(),
            });
            h.frame();
            let zebra = scenario::rect_of("button:inspect.zebra").unwrap();
            assert_eq!(scenario::rect_of("ribbon:reveal"), Some(zebra));
            assert!(h.app.session.status_items().is_empty(), "a search pulse does not activate an effect");
            assert_presentation(&h, "inspect.zebra", false, false);
            h.app.session.execute("inspect.zebra", &json!({})).unwrap();
            h.frame();
            assert_presentation(&h, "inspect.zebra", enabled, enabled);
            let accent = crate::workspace::tab_accent(&Tokens::get(), "INSPECT");
            assert!(button_shapes(&h, "inspect.zebra").iter().any(|shape| matches!(shape, egui::Shape::Rect(rect) if rect.rect == zebra && rect.stroke.width == 2.0 && rect.stroke.color == accent.color.gamma_multiply(h.app.sbox.reveal.as_ref().unwrap().strength(h.ctx.input(|input| input.time), h.ctx.global_style().animation_time > 0.0)))), "the upstream pulse remains painted above the selected background");
            for (id, active) in [("inspect.draft", true), ("inspect.draft", false)] {
                let at = scenario::handle_at(&format!("button:{id}")).unwrap();
                h.frame_with(crate::test_support::pointer(at, true));
                h.frame_with(crate::test_support::pointer(at, false));
                assert_presentation(&h, "inspect.zebra", false, false);
                assert_eq!(h.app.session.status_items().iter().any(|item| item.command == id), active);
                assert_paint(&h, id, active && enabled);
                assert_eq!(scenario::rect_of("ribbon:reveal"), Some(zebra));
            }
            assert!(h.app.session.status_items().is_empty());
            h.app.sbox.reveal = None;
            h.frame();
            assert!(scenario::rect_of("ribbon:reveal").is_none());
        }
    }

    fn open_status_grid(h: &mut Harness, panel: &str) {
        h.app.preferences.ribbon_icon_grid = true;
        h.app.preferences.ribbon_menus_skip_pinned = false;
        h.frames(3);
        h.step(&json!({"click":{"handle":format!("panel:INSPECT:{panel}")}})).unwrap();
        // Exact token colours are checked after the native Area fade, not during its opacity ramp.
        let settle_frames = (h.ctx.global_style().animation_time * 60.0).ceil() as usize + 2;
        h.frames(settle_frames.max(3));
        assert!(scenario::rect_of("toolbar_dropdown").is_some());
    }

    fn close_status_grid(h: &mut Harness, panel: &str) {
        // Use the real header toggle so closing a menu never supplies an idle effect Escape.
        h.step(&json!({"click":{"handle":format!("panel:INSPECT:{panel}")}})).unwrap();
        h.frames(3);
        assert!(scenario::rect_of("toolbar_dropdown").is_none());
    }

    fn status_grid_rect(_h: &Harness, id: &str) -> Rect {
        let at = scenario::handle_at(&format!("dropdown:{id}")).unwrap();
        Rect::from_center_size(at, vec2(ribbon::GRID_CELL, ribbon::GRID_CELL)).shrink(2.0)
    }

    fn assert_status_grid(h: &Harness, id: &str, selected: bool, toggled: Option<egui::accesskit::Toggled>) {
        let rect = status_grid_rect(h, id);
        let t = Tokens::get();
        let mut drawn = Vec::new();
        for shape in &h.shapes {
            shapes(&shape.shape, &mut drawn);
        }
        assert_eq!(
            drawn.iter().any(|shape| matches!(shape, egui::Shape::Rect(s) if s.rect == rect && s.fill == t.selected_bg)),
            selected,
            "{id} grid fill"
        );
        assert_eq!(drawn.iter().any(|shape| matches!(shape, egui::Shape::LineSegment { points, stroke } if stroke.width == 2.0 && stroke.color == t.accent && points[0].y == rect.bottom() - t.space_4 && rect.contains(points[0]))), selected, "{id} grid underline");
        let command = solidcraft_engine::find_command(id).unwrap();
        let label = if toggled.is_some() { persistent_button_name(&h.app, command) } else { h.app.tr_label(command.label).to_string() };
        let node = h
            .output
            .accesskit_update
            .as_ref()
            .and_then(|tree| {
                tree.nodes.iter().find(|(_, node)| {
                    node.role() == egui::accesskit::Role::Button
                        && node.label() == Some(label.as_str())
                        && node.bounds().is_some_and(|b| rect.contains(pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)))
                })
            })
            .map(|(_, node)| node)
            .expect("the actual grid button, distinguished from a same-named pinned ribbon button by bounds");
        assert_eq!(node.toggled(), toggled, "{id} grid accessibility");
    }

    #[test]
    fn persistent_grid_deferred_paint_retains_partial_and_offscreen_scroll_clips() {
        let mut saw_partial = false;
        let mut saw_offscreen = false;
        for offset in [0.0, 24.0, 96.0] {
            let mut h = harness();
            h.app.session.execute("inspect.section", &json!({"plane":"XY","at":4})).unwrap();
            h.app.session.execute("inspect.zebra", &json!({})).unwrap();
            let effects = h.app.session.status_items();
            let ctx = h.ctx.clone();
            let mut expected = Vec::new();
            let mut output = ctx.run_ui(
                egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 240.0))), focused: true, ..Default::default() },
                |ui| {
                    expected.clear(); // Context layout passes never accumulate stale paint expectations.
                    // Leave visible space above the scroll viewport, where a broadened clip leaks.
                    ui.add_space(80.0);
                    let mut paints = Vec::new();
                    egui::ScrollArea::vertical()
                        .id_salt("persistent_grid_clip_regression")
                        .auto_shrink([false, false])
                        .max_height(64.0)
                        .min_scrolled_height(64.0)
                        .vertical_scroll_offset(offset)
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for id in ["inspect.section", "inspect.zebra", "inspect.draft"] {
                                let command = solidcraft_engine::find_command(id).unwrap();
                                let (rect, _) = ui.allocate_exact_size(vec2(ribbon::GRID_CELL, ribbon::GRID_CELL), Sense::hover());
                                let clip = ui.clip_rect().intersect(rect);
                                let _ = dropdown_cell(&h.app, ui, command, rect, false, &mut paints);
                                if id != "inspect.draft" {
                                    expected.push((id, rect.shrink(2.0), clip));
                                }
                            }
                        });
                    // This is deliberately the larger parent Ui, matching the real post-dispatch call.
                    paint_persistent_ribbon(&h.app, ui, paints, "INSPECT", "ribbon_grid");
                },
            );
            output.textures_delta.clear();
            assert_eq!(h.app.session.status_items(), effects, "rendering real active cells never changes their effects");
            for (id, cell, original_clip) in expected {
                let mut backgrounds = Vec::new();
                for clipped in &output.shapes {
                    let mut drawn = Vec::new();
                    shapes(&clipped.shape, &mut drawn);
                    if drawn
                        .iter()
                        .any(|shape| matches!(shape, egui::Shape::Rect(rect) if rect.rect == cell && rect.fill == Tokens::get().selected_bg))
                    {
                        backgrounds.push(clipped.clip_rect);
                    }
                }
                if original_clip.intersect(cell).is_positive() {
                    assert!(!backgrounds.is_empty(), "{id} actually paints its visible selected fragment at offset {offset}");
                    assert!(
                        backgrounds.iter().all(|clip| *clip == original_clip),
                        "{id} retains its original cell/scroll clip, not the parent clip, at offset {offset}"
                    );
                    saw_partial |= !original_clip.contains_rect(cell);
                } else {
                    assert!(
                        backgrounds.iter().all(|clip| !clip.intersect(cell).is_positive()),
                        "offscreen {id} must not paint into the space above its scroll viewport"
                    );
                    saw_offscreen = true;
                }
            }
        }
        assert!(saw_partial, "the native ScrollArea must actually partly clip an active cell");
        assert!(saw_offscreen, "the native ScrollArea must actually scroll an active cell out of view");
    }

    #[test]
    fn persistent_grid_uses_actual_section_and_analysis_with_independent_compatibility_preferences() {
        for section in [false, true] {
            for ribbon in [false, true] {
                let mut h = harness();
                h.app.preferences.viewport_analysis_status = false;
                h.app.preferences.viewport_section_status = section;
                h.app.preferences.persistent_ribbon_pressed = ribbon;
                h.app.session.execute("inspect.zebra", &json!({"stripes":14})).unwrap();
                h.app.session.execute("inspect.section", &json!({"plane":"XY","at":4})).unwrap();
                let effects = h.app.session.status_items();
                open_status_grid(&mut h, "SECTION");
                assert_status_grid(&h, "inspect.section", section && ribbon, (section && ribbon).then_some(egui::accesskit::Toggled::True));
                assert_eq!(h.app.session.status_items(), effects);
                close_status_grid(&mut h, "SECTION");
                open_status_grid(&mut h, "SURFACE QUALITY");
                assert_status_grid(&h, "inspect.zebra", ribbon, ribbon.then_some(egui::accesskit::Toggled::True));
                assert_eq!(h.app.session.status_items(), effects, "presentation preferences and menu navigation never mutate effects");
                close_status_grid(&mut h, "SURFACE QUALITY");
            }
        }
    }

    #[test]
    fn persistent_grid_tracks_all_six_analysis_replacements_queries_and_failed_activation() {
        let mut h = harness();
        h.app.session.execute("inspect.section", &json!({"plane":"YZ","at":3})).unwrap();
        let cut = h.app.session.section;
        for id in PERSISTENT_COMMANDS.iter().copied().filter(|id| *id != "inspect.section") {
            h.app.session.execute(id, &json!({})).unwrap();
            let before = h.app.session.status_items();
            h.app.session.execute("view.status_items", &json!({})).unwrap();
            assert!(h.app.session.execute("inspect.draft", &json!({"pull":[0,0,0]})).is_err());
            assert_eq!(h.app.session.status_items(), before);
            let panel = if matches!(id, "inspect.draft" | "inspect.accessibility") { "CHECK" } else { "SURFACE QUALITY" };
            open_status_grid(&mut h, panel);
            assert_status_grid(&h, id, true, Some(egui::accesskit::Toggled::True));
            assert_eq!(h.app.session.section, cut, "analysis replacement leaves Section independent");
            close_status_grid(&mut h, panel);
        }
    }

    #[test]
    fn persistent_grid_keeps_native_keyboard_focus_and_active_tooltip() {
        let mut h = harness();
        h.app.session.execute("inspect.section", &json!({"plane":"XY","at":4})).unwrap();
        open_status_grid(&mut h, "SECTION");
        h.step(&json!({"key":"ArrowRight"})).unwrap();
        h.frames(3);
        assert!(scenario::rect_of("ribbon_grid:focus:inspect.section").is_some());
        assert_status_grid(&h, "inspect.section", true, Some(egui::accesskit::Toggled::True));
        h.step(&json!({"move":{"handle":"dropdown:inspect.section"}})).unwrap();
        h.frames(100);
        let command = solidcraft_engine::find_command("inspect.section").unwrap();
        let hint = persistent_hint(&h.app, command, true).unwrap();
        let mut drawn = Vec::new();
        for shape in &h.shapes {
            shapes(&shape.shape, &mut drawn);
        }
        assert!(
            drawn.iter().any(|shape| matches!(shape, egui::Shape::Text(text) if text.galley.text() == hint)),
            "the actual grid uses the same native turn-off tooltip"
        );
        assert!(h.app.session.section.is_some());
    }

    #[test]
    fn persistent_grid_clear_uses_post_dispatch_session_without_stale_paint_or_cross_effect_clear() {
        for (id, panel) in [("inspect.section", "SECTION"), ("inspect.zebra", "SURFACE QUALITY")] {
            let mut h = harness();
            h.app.session.execute("inspect.zebra", &json!({})).unwrap();
            h.app.session.execute("inspect.section", &json!({"plane":"XY","at":4})).unwrap();
            open_status_grid(&mut h, panel);
            assert_status_grid(&h, id, true, Some(egui::accesskit::Toggled::True));
            let rect = status_grid_rect(&h, id);
            let at = scenario::handle_at(&format!("dropdown:{id}")).unwrap();
            let before = h.app.session.status_items();
            h.frame_with(crate::test_support::pointer(at, true));
            assert_eq!(h.app.session.status_items(), before, "pointer down is not a command activation");
            h.frame_with(crate::test_support::pointer(at, false));
            // Check the release frame, with no settling frame: both paint paths resolve after dispatch.
            assert_paint(&h, id, false);
            let mut drawn = Vec::new();
            for shape in &h.shapes {
                shapes(&shape.shape, &mut drawn);
            }
            let t = Tokens::get();
            // The pointer still hovers this cell: native hover may share the selected fill.
            // Persistence is distinguished by the underline, identity and native toggled state.
            assert!(
                drawn.iter().any(|shape| matches!(shape, egui::Shape::Rect(s) if s.rect == rect && s.fill == t.accent_soft)),
                "{id} retains its normal hovered fill on release"
            );
            assert!(!drawn.iter().any(|shape| matches!(shape, egui::Shape::LineSegment { points, stroke } if stroke.width == 2.0 && stroke.color == t.accent && points[0].y == rect.bottom() - t.space_4 && rect.contains(points[0]))), "{id} has no persistent underline on release");
            assert!(scenario::rect_of(&format!("ribbon_grid:pressed:{id}")).is_none(), "{id} has no published pressed identity on release");
            let command = solidcraft_engine::find_command(id).unwrap();
            let label = persistent_button_name(&h.app, command);
            let node = h
                .output
                .accesskit_update
                .as_ref()
                .and_then(|tree| {
                    tree.nodes.iter().find(|(_, node)| {
                        node.role() == egui::accesskit::Role::Button
                            && node.label() == Some(label.as_str())
                            && node.bounds().is_some_and(|b| rect.contains(pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)))
                    })
                })
                .map(|(_, node)| node)
                .expect("the actual cleared grid button, distinguished from its pinned ribbon button by bounds");
            assert_eq!(node.toggled(), Some(egui::accesskit::Toggled::False), "{id} grid accessibility is off on the release frame");
            assert_eq!(h.app.session.section.is_some(), id != "inspect.section");
            assert_eq!(h.app.session.analysis.is_some(), id != "inspect.zebra");
        }
    }

    #[test]
    fn section_persistent_ribbon_uses_independent_status_preference_and_active_toggle() {
        let mut h = harness();
        h.app.session.execute("inspect.zebra", &json!({})).unwrap();
        h.app.session.execute("inspect.section", &json!({"plane":"XY","at":4})).unwrap();
        h.frame();
        assert_paint(&h, "inspect.section", true);
        assert_paint(&h, "inspect.zebra", true);
        h.app.preferences.viewport_analysis_status = false;
        h.frame();
        assert_paint(&h, "inspect.section", true);
        h.app.preferences.viewport_section_status = false;
        h.frame();
        assert_paint(&h, "inspect.section", false);
        assert!(persistent_hint(&h.app, solidcraft_engine::find_command("inspect.section").unwrap(), true).is_none());
        assert!(h.app.session.section.is_some());
        h.app.preferences.viewport_section_status = true;
        h.app.preferences.persistent_ribbon_pressed = false;
        h.frame();
        assert_paint(&h, "inspect.section", false);
        h.app.preferences.persistent_ribbon_pressed = true;
        h.frames(2);
        tap(&mut h, "inspect.section");
        assert!(h.app.session.section.is_none() && h.app.session.analysis.is_some());
        assert_paint(&h, "inspect.section", false);
        assert_paint(&h, "inspect.zebra", true);
    }

    #[test]
    fn persistent_commands_derive_all_seven_programmatic_effects_and_clear_without_residue() {
        let mut h = harness();
        for id in PERSISTENT_COMMANDS {
            assert!(!persistent_pressed(&h.app, id));
            h.app.session.execute(id, &if *id == "inspect.section" { json!({"plane":"XY"}) } else { json!({}) }).unwrap();
            h.frame(); // Exactly the first output following the engine mutation.
            assert_eq!(h.app.session.status_items().iter().map(|s| s.command.as_str()).collect::<Vec<_>>(), [*id]);
            for c in PERSISTENT_COMMANDS {
                assert_paint(&h, c, c == id);
            }
            let before = h.app.session.status_items();
            let revision = h.app.session.revision;
            h.frame();
            assert_eq!(h.app.session.status_items(), before);
            assert_eq!(h.app.session.revision, revision);
            h.app.session.execute(id, &json!({"clear":true})).unwrap();
            h.frame();
            assert_paint(&h, id, false);
        }
        for id in ["inspect.minimum_radius", "solid.extrude"] {
            assert!(!persistent_command(id));
        }
        h.app.session.execute("inspect.zebra", &json!({})).unwrap();
        let before = h.app.session.status_items();
        assert!(h.app.session.execute("inspect.draft", &json!({"pull":[0,0,0]})).is_err());
        h.frame();
        assert_eq!(h.app.session.status_items(), before);
        assert_paint(&h, "inspect.zebra", true);
    }

    #[test]
    fn persistent_replacement_and_toggle_are_correct_in_the_click_release_frame_both_directions() {
        for labels in [true, false] {
            let mut h = harness();
            h.app.preferences.ribbon_labels = labels;
            crate::toolbar_custom::reorder(&mut h.app, "INSPECT", "inspect.draft", "SURFACE QUALITY", Some("inspect.zebra"));
            h.frames(2);
            for id in ["inspect.zebra", "inspect.draft", "inspect.zebra", "inspect.continuity", "inspect.environment_map"] {
                tap(&mut h, id); // No settling frame after release.
                assert_eq!(h.app.session.status_items().iter().map(|s| s.command.as_str()).collect::<Vec<_>>(), [id]);
                for c in PERSISTENT_COMMANDS {
                    assert_paint(&h, c, *c == id);
                }
                let c = solidcraft_engine::find_command(id).unwrap();
                assert_eq!(
                    persistent_hint(&h.app, c, true),
                    Some(format!("Turn off {} · Esc", h.app.tr_label(crate::workspace::ribbon_button_label(c))))
                );
            }
            tap(&mut h, "inspect.environment_map");
            assert!(h.app.session.status_items().is_empty());
            assert_paint(&h, "inspect.environment_map", false);
            tap(&mut h, "inspect.continuity");
            tap(&mut h, "inspect.continuity");
            assert!(h.app.session.status_items().is_empty());
            assert_paint(&h, "inspect.continuity", false);
        }
    }

    #[test]
    fn persistent_presentation_preference_preserves_old_help_and_legacy_two_effects() {
        let mut h = harness();
        h.call("ui.prefs", json!({"single_surface_analysis":false}));
        h.app.session.execute("inspect.zebra", &json!({})).unwrap();
        h.app.session.execute("inspect.continuity", &json!({})).unwrap();
        h.frame();
        assert_eq!(h.app.session.status_items().len(), 2);
        assert_paint(&h, "inspect.zebra", true);
        assert_paint(&h, "inspect.continuity", true);
        let before = h.app.session.status_items();
        h.app.preferences.persistent_ribbon_pressed = false;
        h.frame();
        for id in PERSISTENT_COMMANDS {
            assert_paint(&h, id, false);
        }
        assert_eq!(h.app.session.status_items(), before);
        let label = h.app.tr_label(solidcraft_engine::find_command("inspect.zebra").unwrap().info(&h.app.session).label);
        let old_node = h
            .output
            .accesskit_update
            .as_ref()
            .and_then(|tree| tree.nodes.iter().find(|(_, n)| n.label() == Some(label)))
            .map(|(_, n)| n)
            .expect("old neutral accessible button label");
        assert_eq!(old_node.toggled(), None, "the old presentation makes no false inactive-effect announcement");
        assert!(persistent_hint(&h.app, solidcraft_engine::find_command("inspect.zebra").unwrap(), true).is_none());
        h.app.preferences.persistent_ribbon_pressed = true;
        h.frame();
        assert_paint(&h, "inspect.zebra", true);
        assert_paint(&h, "inspect.continuity", true);
        h.app.last_command = Some(("inspect.draft".into(), "Draft".into()));
        h.frame();
        assert_paint(&h, "inspect.draft", false);
        assert!(persistent_hint(&h.app, solidcraft_engine::find_command("inspect.zebra").unwrap(), false).is_none());
    }

    #[test]
    fn persistent_selected_shapes_and_real_semibold_survive_every_colour_vision_mode() {
        let mut h = harness();
        h.app.session.execute("inspect.zebra", &json!({})).unwrap();
        for dark in [false, true] {
            for vision in ["off", "protan", "deutan", "tritan"] {
                h.call("ui.prefs", json!({"theme":if dark { "dark" } else { "light" },"color_vision":vision}));
                h.frame();
                assert_paint(&h, "inspect.zebra", true);
                assert_paint(&h, "inspect.curvature_map", false);
            }
        }
        let painter = h.ctx.layer_painter(egui::LayerId::background());
        let plain = ribbon::button_galley_weight(&painter, "Zebra", 150.0, Tokens::get().text, false);
        let heavy = ribbon::button_galley_weight(&painter, "Zebra", 150.0, Tokens::get().text, true);
        assert!((plain.size().x - heavy.size().x).abs() > 0.1, "actual 600-weight face has different glyph metrics");
    }

    #[test]
    fn persistent_keyboard_focus_uses_native_response_and_clears_with_space() {
        let mut h = harness();
        h.app.session.execute("inspect.zebra", &json!({})).unwrap();
        h.frame();
        let at = scenario::handle_at("button:inspect.zebra").unwrap();
        // A pointer press establishes the real existing response's keyboard focus.
        h.frame_with(crate::test_support::pointer(at, true));
        h.frame_with(crate::test_support::pointer(at, false));
        let id = h.ctx.memory(|m| m.focused()).expect("native ribbon response is focusable");
        assert!(h.app.session.status_items().is_empty());
        h.ctx.memory_mut(|m| m.request_focus(id));
        h.frame_with(crate::test_support::key(egui::Key::Space));
        assert_eq!(h.app.session.status_items().len(), 1);
        assert_paint(&h, "inspect.zebra", true);
        assert!(scenario::rect_of("ribbon:focus:inspect.zebra").is_some());
        let node = h
            .output
            .accesskit_update
            .as_ref()
            .and_then(|tree| tree.nodes.iter().find(|(_, n)| n.label() == Some("Zebra")))
            .map(|(_, n)| n)
            .expect("accessible native Zebra button");
        assert_eq!(node.role(), egui::accesskit::Role::Button);
        assert_eq!(node.toggled(), Some(egui::accesskit::Toggled::True));
    }

    #[test]
    fn persistent_tooltip_is_the_exact_translated_visible_label_and_old_help_is_reachable() {
        let mut h = harness();
        h.app.session.execute("inspect.zebra", &json!({})).unwrap();
        for language in ["en", "de", "ja"] {
            h.call("ui.prefs", json!({"language":language}));
            let command = solidcraft_engine::find_command("inspect.zebra").unwrap();
            let name = h.app.tr_label(crate::workspace::ribbon_button_label(command));
            let expected = solidcraft_ui_text::Locale::for_language(language).format("ui.viewport_status_clear_hint", &[("name", name)]);
            assert_eq!(persistent_hint(&h.app, command, true), Some(expected.clone()));
            let at = scenario::handle_at("button:inspect.zebra").unwrap();
            h.frame_with(vec![egui::Event::PointerMoved(at)]);
            h.frames(100);
            let mut all = Vec::new();
            for shape in &h.shapes {
                shapes(&shape.shape, &mut all);
            }
            assert!(all.iter().any(|s| matches!(s, egui::Shape::Text(t) if t.galley.text() == expected)), "actual tooltip {expected}");
        }
        h.app.preferences.persistent_ribbon_pressed = false;
        h.frame_with(vec![egui::Event::PointerMoved(egui::Pos2::ZERO)]);
        h.frame_with(vec![egui::Event::PointerMoved(scenario::handle_at("button:inspect.zebra").unwrap())]);
        h.frames(100);
        assert!(persistent_hint(&h.app, solidcraft_engine::find_command("inspect.zebra").unwrap(), true).is_none());
        assert_paint(&h, "inspect.zebra", false);
    }

    #[test]
    fn persistent_dropdown_clear_keeps_native_pin_and_command_routes() {
        let mut h = harness();
        h.app.session.execute("inspect.environment_map", &json!({})).unwrap();
        h.frame();
        h.step(&json!({"click":{"handle":"panel:INSPECT:SURFACE QUALITY"}})).unwrap();
        assert!(scenario::handle_at("dropdown:inspect.environment_map").is_some());
        h.step(&json!({"click":{"handle":"dropdown:inspect.environment_map"}})).unwrap();
        assert!(h.app.session.status_items().is_empty());
        assert_paint(&h, "inspect.environment_map", false);
    }

    #[test]
    fn persistent_registration_preserves_live_continuity_tool_without_a_status_item() {
        let mut h = harness();
        h.app.start("inspect.continuity");
        assert_eq!(h.app.tool.as_ref().map(|tool| tool.cmd), Some("inspect.continuity"));
        h.app.session.execute("inspect.continuity", &json!({"clear":true})).unwrap();
        h.frame();
        assert!(h.app.session.status_items().is_empty());
        assert_eq!(scenario::count_of("ribbon:pressed"), Some(0.0));
        assert_presentation(&h, "inspect.continuity", false, false);
        let r = scenario::rect_of("button:inspect.continuity").unwrap();
        let accent = crate::workspace::tab_accent(&Tokens::get(), "INSPECT");
        assert!(
            button_shapes(&h, "inspect.continuity")
                .iter()
                .any(|s| matches!(s, egui::Shape::Rect(s) if s.rect == r && s.fill == accent.soft && s.stroke.color == accent.color)),
            "the real live tool retains its transient indication"
        );
        h.app.session.execute("inspect.continuity", &json!({})).unwrap();
        h.frame();
        assert_paint(&h, "inspect.continuity", true);
        // A dialog's last-command fallback must not invent an analysis tool.
        h.app.tool = None;
        h.app.start("solid.box");
        assert!(h.app.dialog.is_some());
        h.app.ui.tab = "INSPECT".into();
        h.app.last_command = Some(("inspect.zebra".into(), "Zebra".into()));
        h.frame();
        assert_paint(&h, "inspect.zebra", false);
        let r = scenario::rect_of("button:inspect.zebra").unwrap();
        assert!(
            !button_shapes(&h, "inspect.zebra")
                .iter()
                .any(|s| matches!(s, egui::Shape::Rect(s) if s.rect == r && s.fill == accent.soft && s.stroke.color == accent.color)),
            "last_command is not a live analysis tool"
        );
    }

    #[test]
    fn persistent_click_clear_and_replacement_remove_old_tool_paint_in_the_release_frame() {
        fn no_tool_paint(h: &Harness) {
            let r = scenario::rect_of("button:inspect.continuity").unwrap();
            let accent = crate::workspace::tab_accent(&Tokens::get(), "INSPECT");
            assert!(
                !button_shapes(h, "inspect.continuity")
                    .iter()
                    .any(|s| matches!(s, egui::Shape::Rect(s) if s.rect == r && s.fill == accent.soft && s.stroke.color == accent.color)),
                "no captured old live-tool fill/border after dispatch"
            );
            assert!(scenario::handle_at("ribbon:pressed:inspect.continuity").is_none());
            assert_paint(h, "inspect.continuity", false);
        }
        for labels in [true, false] {
            let mut h = harness();
            h.app.preferences.ribbon_labels = labels;
            crate::toolbar_custom::reorder(&mut h.app, "INSPECT", "inspect.continuity", "SURFACE QUALITY", Some("inspect.environment_map"));
            h.frames(2);
            tap(&mut h, "inspect.continuity");
            assert!(h.app.tool.is_some());
            tap(&mut h, "inspect.continuity");
            assert!(h.app.tool.is_none() && h.app.session.status_items().is_empty());
            no_tool_paint(&h);
            tap(&mut h, "inspect.continuity");
            tap(&mut h, "inspect.environment_map");
            assert!(h.app.tool.is_none());
            assert_eq!(h.app.session.status_items()[0].command, "inspect.environment_map");
            no_tool_paint(&h);
            assert_paint(&h, "inspect.environment_map", true);
        }
    }
}

#[cfg(test)]
mod quick_access_tests {
    use crate::scenario::{Harness, handle_at, rect_of};
    use serde_json::json;

    fn pick(h: &mut Harness, handle: &str, item: &str) {
        h.frames(3);
        h.step(&json!({"click":{"handle": handle},"button":"right"})).unwrap();
        h.frames(3);
        assert!(h.app.viewport.context_menu.is_some(), "{handle}: the right-click opens the command's menu");
        h.step(&json!({"widget": item})).unwrap_or_else(|e| panic!("{handle}: {item}: {e}"));
        h.frames(3);
    }

    /// The quick-access bar (#10223) is empty and not drawn by default; a ribbon button's menu
    /// adds its command after Save, Undo and Redo, a drag along the bar reorders the buttons, a
    /// click starts the command, and the button's own menu removes it. The list is kept with the
    /// toolbar's saved customisations.
    #[test]
    fn ribbon_commands_are_added_reordered_started_and_removed_from_the_quick_access_bar() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1600,"height":1000}));
        h.frames(3);
        assert!(h.app.toolbar_custom.quick_access.is_empty(), "empty by default");
        assert!(rect_of("appbar:quick_access").is_none(), "and not drawn");

        pick(&mut h, "button:solid.extrude", "Add to Quick Access");
        pick(&mut h, "button:solid.fillet", "Add to Quick Access");
        assert_eq!(h.app.toolbar_custom.quick_access, ["solid.extrude", "solid.fillet"]);
        h.frames(2);
        let bar = rect_of("appbar:quick_access").unwrap();
        let redo = rect_of("appbar:redo").unwrap();
        assert!(bar.left() >= redo.right() && bar.center().y == redo.center().y, "after Redo: {bar:?} {redo:?}");
        let extrude = handle_at("appbar:quick:solid.extrude").unwrap();
        let fillet = handle_at("appbar:quick:solid.fillet").unwrap();
        assert!(extrude.x < fillet.x, "in the order they were added");

        // Dragged before Extrude, Fillet comes first.
        h.step(&json!({"drag": [[fillet.x, fillet.y], [extrude.x - 10.0, extrude.y]]})).unwrap();
        h.frames(3);
        assert_eq!(h.app.toolbar_custom.quick_access, ["solid.fillet", "solid.extrude"]);

        // Saved with the toolbar and read back.
        let saved = h.app.toolbar_custom.prefs();
        let mut reloaded = crate::toolbar_custom::ToolbarCustom::default();
        reloaded.load(&saved);
        assert_eq!(reloaded.quick_access, ["solid.fillet", "solid.extrude"]);
        reloaded.load(&json!({"version": 3, "quick_access": ["solid.extrude", "solid.extrude", 7, "", "no.such.command"]}));
        assert_eq!(reloaded.quick_access, ["solid.extrude", "no.such.command"], "each once; unknown ids kept but not drawn");

        // A click starts the command as its ribbon button does.
        h.step(&json!({"click":{"handle":"appbar:quick:solid.extrude"}})).unwrap();
        h.frames(2);
        assert_eq!(
            h.app.tool.as_ref().map(|t| t.cmd.to_string()).or_else(|| h.app.last_command.as_ref().map(|(id, _)| id.clone())).as_deref(),
            Some("solid.extrude")
        );
        h.step(&json!({"key":"Escape"})).unwrap();
        h.frames(2);

        // The button's own menu takes it off; the last one gone, the bar goes too.
        pick(&mut h, "appbar:quick:solid.extrude", "Remove from Quick Access");
        assert_eq!(h.app.toolbar_custom.quick_access, ["solid.fillet"]);
        pick(&mut h, "button:solid.fillet", "Remove from Quick Access");
        assert!(h.app.toolbar_custom.quick_access.is_empty());
        h.frames(2);
        assert!(rect_of("appbar:quick_access").is_none());
        assert!(h.app.session.doc.features.is_empty(), "nothing in the design changed");
    }

    /// A quick-access click starts exactly the pinned variant and leaves the family's remembered
    /// ribbon choice alone.
    #[test]
    fn a_quick_access_click_on_a_family_variant_keeps_the_remembered_ribbon_choice() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1600,"height":1000}));
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.settle();
        h.app.start("sketch.rectangle.three_point");
        h.settle();
        h.step(&json!({"key":"Escape"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.preferences.sketch_variants.resolve("sketch.rectangle.two_point"), "sketch.rectangle.three_point");

        h.app.toolbar_custom.quick_access = vec!["sketch.rectangle.center".to_string()];
        h.frames(3);
        h.step(&json!({"click":{"handle":"appbar:quick:sketch.rectangle.center"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.tool.as_ref().map(|t| t.cmd.to_string()).as_deref(), Some("sketch.rectangle.center"));
        assert_eq!(
            h.app.preferences.sketch_variants.resolve("sketch.rectangle.two_point"),
            "sketch.rectangle.three_point",
            "the ribbon's Rectangle button still starts its remembered choice"
        );
    }
}
