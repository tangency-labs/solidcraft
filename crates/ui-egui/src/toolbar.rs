//! Application bar, the workspace toolbar (tabs and panels) and keyboard shortcuts.

use crate::fonts::RichTextExt as _;
use crate::icons::glyphs::pin_icon;
use crate::ui_kit::{self, Container, Edge, Surface};
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;

use crate::theme::Tokens;
use crate::{SolveApp, icons};

pub use crate::workspace::TABS;

/// Legacy paint arguments and non-SVG affordances follow the same semantic palette as the icon.
/// The requested icon id determines its meaning on ribbons, menus and drag previews alike.
fn command_accent(icon: &str) -> Color32 {
    crate::icon_svg::primary(icon, crate::theme::is_dark(), crate::theme::vision())
}

/// Accent for a tab or a panel with a more specific semantic role.
fn panel_accent(t: &Tokens, tab: &str, panel: &str) -> Color32 {
    if panel == "CONSTRAIN" {
        return t.glyph_text;
    }
    crate::theme::TabAccent::for_tab(t, tab).color
}

/// Panels shown on each tab (in order). Commands come from the registry by (tab, panel).
pub fn panels(tab: &str) -> Vec<&'static str> {
    crate::workspace::panels(tab)
}

/// How a panel is drawn on a toolbar that may be too narrow for every button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// Its first buttons (all of them, or the ones that fit); the panel's drop-down lists every
    /// command.
    Buttons,
    /// One button that opens the panel's drop-down (which lists every command, pinned or not).
    Collapsed,
    /// In the » menu at the end of the toolbar.
    More,
}

/// A ribbon button's preferred and minimum widths. The constraint gallery is a fixed-width
/// block; ordinary buttons narrow as their labels wrap and truncate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonSize {
    pub width: f32,
    pub min_width: f32,
}

/// One panel as [`fit`] sees it, in button order (the gallery, if present, is last).
#[derive(Clone, Debug, PartialEq)]
pub struct PanelSize {
    pub buttons: Vec<ButtonSize>,
}

/// Where [`fit`] put a panel: its first buttons and their individual fitted widths.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub fit: Fit,
    pub shown: usize,
    pub widths: Vec<f32>,
}

impl Plan {
    fn full(panel: &PanelSize) -> Self {
        Self { fit: Fit::Buttons, shown: panel.buttons.len(), widths: panel.buttons.iter().map(|b| b.width).collect() }
    }

    fn more() -> Self {
        Self { fit: Fit::More, shown: 0, widths: Vec::new() }
    }

    /// The panel's width on the toolbar (0 in the » menu).
    pub fn width(&self) -> f32 {
        match self.fit {
            Fit::More => 0.0,
            Fit::Collapsed => COLLAPSED_WIDTH,
            Fit::Buttons => panel_width(self.widths.get(..self.shown).unwrap_or_default()),
        }
    }
}

/// A panel's width with these individual button widths.
pub fn panel_width(buttons: &[f32]) -> f32 {
    (buttons.iter().sum::<f32>() + buttons.len().saturating_sub(1) as f32 * BUTTON_GAP + 8.0).max(COLLAPSED_WIDTH)
}

/// A collapsed panel's width: one button's panel.
pub const COLLAPSED_WIDTH: f32 = 72.0;

/// The gap after each panel (its separator).
const PANEL_GAP: f32 = 1.0;
pub const TAB_ROW_HEIGHT: f32 = 30.0;
/// Distance from the ribbon card's edges to the first tab and the search field.
const RIBBON_TAB_INSET: f32 = 4.0;
pub const GROUP_HEIGHT: f32 = 86.0;
pub const RIBBON_HEIGHT: f32 = TAB_ROW_HEIGHT + GROUP_HEIGHT + 1.0;
pub(crate) const WORKSPACE_WIDTH: f32 = 88.0;
const BUTTON_HEIGHT: f32 = 60.0;
const ICON_BUTTON_HEIGHT: f32 = 36.0;
const ICON_GROUP_HEIGHT: f32 = 62.0;

/// The same height drives shell reservations and ribbon painting in either layout.
pub(crate) fn ribbon_height(labels: bool) -> f32 {
    if labels { RIBBON_HEIGHT } else { TAB_ROW_HEIGHT + ICON_GROUP_HEIGHT + 1.0 }
}
const BUTTON_GAP: f32 = 3.0;
const BUTTON_MIN_WIDTH: f32 = 54.0;
const BUTTON_MAX_WIDTH: f32 = 76.0;
const BUTTON_LABEL_PADDING: f32 = 10.0;
const SECTION_HEIGHT: f32 = 20.0;

/// The » button's room at the end of the panels, kept only while some panel is in it.
pub const MORE_ROOM: f32 = 40.0;

/// Fit panels into `room`, giving up as little as it takes, one small step at a time:
/// 1. buttons narrow from their measured label widths to their minimum, wrapping and truncating;
/// 2. panels hand their last buttons to their drop-down one at a time, the panel showing the
///    most buttons first (the rightmost of equals), down to one button (collapsed);
/// 3. the last panels go into the » menu, and any room that frees goes back to buttons.
///
/// Each removal frees at most one button (or the fixed-width constraint gallery), so the toolbar
/// never leaves a larger gap while something is hidden. The result depends only on `room`
/// (no state from earlier frames), so it never flickers. Nothing is dropped.
pub fn fit(panels: &[PanelSize], room: f32) -> Vec<Plan> {
    let room = if room.is_finite() { room } else { 0.0 };
    if let Some(plans) = shrink(panels, room) {
        return plans;
    }
    // 3. The last panels into the » menu (which takes its own room), as few as it takes; the
    // ones left keep as many buttons as fit.
    for kept in (0..panels.len()).rev() {
        if let Some(mut plans) = shrink(panels.get(..kept).unwrap_or_default(), room - MORE_ROOM) {
            plans.extend(panels.iter().skip(kept).map(|_| Plan::more()));
            return plans;
        }
    }
    panels.iter().map(|_| Plan::more()).collect()
}

/// The room plans take on the toolbar (each panel and its separator).
fn used(plans: &[Plan]) -> f32 {
    plans.iter().filter(|p| p.fit != Fit::More).map(|p| p.width() + PANEL_GAP).sum()
}

/// Steps 1 and 2 of [`fit`]: every panel on the toolbar, or None when even one button each
/// doesn't fit.
fn shrink(panels: &[PanelSize], room: f32) -> Option<Vec<Plan>> {
    let mut plans: Vec<Plan> = panels.iter().map(Plan::full).collect();
    if used(&plans) <= room {
        return Some(plans);
    }
    // 1. Narrow together in one-pixel steps, keeping each button's measured width until it
    // reaches its minimum. The gallery's minimum equals its full width.
    for reduction in 1..=((BUTTON_MAX_WIDTH - BUTTON_MIN_WIDTH) as usize) {
        for (plan, panel) in plans.iter_mut().zip(panels) {
            for (width, button) in plan.widths.iter_mut().zip(&panel.buttons) {
                *width = (button.width - reduction as f32).max(button.min_width);
            }
        }
        if used(&plans) <= room {
            return Some(plans);
        }
    }
    // 2. One button at a time into the drop-downs: from the panel showing the most (the
    // rightmost of equals), down to one button each.
    while used(&plans) > room {
        let pick = plans
            .iter()
            .enumerate()
            .filter(|(_, p)| p.fit == Fit::Buttons && (p.shown > 1 || p.width() > COLLAPSED_WIDTH))
            .max_by(|(i, a), (j, b)| a.shown.cmp(&b.shown).then(i.cmp(j)));
        let (i, _) = pick?;
        if let Some(p) = plans.get_mut(i) {
            p.shown = p.shown.saturating_sub(1).max(1);
            if p.shown == 1 {
                p.fit = Fit::Collapsed;
            }
        }
    }
    Some(plans)
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

/// Room for the version between Report a bug and the caption buttons, at the least.
const RIGHT_GROUP: f32 = 60.0;
const CLUSTER_BUTTONS: f32 = 108.0;

/// The least room the document tabs keep when a long version takes its extra width: one tab and
/// the + button ([`crate::documents::layout`]).
const TABS_KEPT: f32 = 124.0;

/// The room right of Report a bug for the version (`version_width`, measured), or None where the
/// bar shows none. The version sits 12 points from the caption buttons with a gap before the
/// button, so a longer one (`v0.21.0-rc.1`) moves the button left instead of covering it. Its
/// width over [`RIGHT_GROUP`] comes only from the document tabs' spare room: where it would leave
/// them less than [`TABS_KEPT`], the version is hidden, as it is on a narrow bar.
fn version_room(bar: Rect, captions: f32, tabs_start: f32, version_width: f32) -> Option<f32> {
    if bar.width() < 640.0 {
        return None;
    }
    let need = 12.0 + version_width + 8.0;
    let room = RIGHT_GROUP.max(need);
    // The tabs end 8 points before the cluster, with Report a bug as an icon at the least.
    let tabs = bar.right() - captions - room - 28.0 - CLUSTER_BUTTONS - 8.0 - tabs_start;
    (need <= RIGHT_GROUP || tabs >= TABS_KEPT).then_some(room)
}

/// The centre of Help, between Settings and Report a bug.
fn help_x(report: Rect) -> f32 {
    report.left() - 18.0
}

/// The Report a bug button: labelled when there is room for its text (`label_width`, measured in
/// the current language), otherwise just the icon. `version_width` is the measured version.
fn report_rect(bar: Rect, captions: f32, tabs_start: f32, label_width: f32, version_width: f32) -> (Rect, bool) {
    let right = bar.right() - captions - version_room(bar, captions, tabs_start, version_width).unwrap_or(8.0);
    // Icon, text and a margin that mirrors the icon side.
    let full = REPORT_TEXT_X + label_width + 11.0;
    let labeled = right - CLUSTER_BUTTONS - tabs_start >= 216.0 + full;
    let width = if labeled { full } else { 28.0 };
    (Rect::from_min_size(pos2(right - width, bar.center().y - 13.0), vec2(width, 26.0)), labeled)
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
    let response = ui.interact(rect, ui.id().with(("sc_offer_pill", handle)), Sense::click());
    let name = label.map(|(text, _)| text).or(labels.first().copied()).unwrap_or_default().to_string();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &name));
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

/// Where the Report a bug text starts, from the button's left edge.
const REPORT_TEXT_X: f32 = 29.0;

/// The application bar's height: the window's title bar on desktop builds.
pub const APP_BAR_HEIGHT: f32 = 34.0;

pub fn app_bar(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let app_bar = Container::new(ui.ctx(), Surface::ApplicationBar).edge("sc_appbar", Edge::Top);
    let frame = Container::new(ui.ctx(), Surface::ApplicationBar).edge_frame(t.app_bar);
    app_bar.exact_size(APP_BAR_HEIGHT).show_separator_line(false).frame(frame).show(ui, |ui| {
        let r = ui.max_rect();
        // The bar is the window's title bar: its empty space moves the window.
        if app.custom_titlebar || app.integrated_titlebar {
            crate::titlebar::drag_area(ui, r);
        }
        let left = r.left() + if app.integrated_titlebar { crate::titlebar::TRAFFIC_LIGHTS } else { 0.0 };
        // Keep the current header controls and product name when merging the new mark.
        let compact_actions = r.right() - left < 640.0;
        // Brand mark: the SolidCraft logo, drawn in code from the SVG's own coordinates
        // (viewBox 357 x 412). Same 20 px height and centre as the old block.
        {
            let c = pos2(left + 20.0, r.center().y);
            crate::icons::glyphs::brand_mark(ui.painter(), c);
        }
        if !compact_actions {
            ui.painter().text(
                pos2(left + 36.0, r.center().y),
                Align2::LEFT_CENTER,
                "SolidCraft",
                crate::fonts::semibold(t.font_heading),
                t.app_bar_text,
            );
        }
        let mut x = left + if compact_actions { 56.0 } else { 128.0 };
        let home = Rect::from_center_size(pos2(x - 4.0, r.center().y), vec2(32.0, 32.0));
        let idle = !app.file_load.busy();
        let home_response = ui.interact(home, ui.id().with("appbar_home"), if idle { Sense::click() } else { Sense::hover() });
        let home_tip = app.tr("ui.start_page");
        home_response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, idle, home_tip));
        crate::scenario::publish_handle("appbar:home", home.center());
        crate::scenario::publish_handle("titlebar:home", home.center());
        // Compatibility for scenarios that used the former Home tab.
        crate::scenario::publish_target_as("tab:home", "titlebar:home", home);
        crate::scenario::publish_rect("appbar:home", home);
        if idle && (home_response.hovered() || home_response.has_focus()) {
            ui.painter().rect_filled(home.shrink(3.0), t.radius_sm, t.app_bar_hover_button);
        }
        icons::paint_app_bar(ui.painter(), home.shrink(7.0), "home", if idle { 1.0 } else { 0.35 });
        if home_response.on_hover_text(home_tip).clicked() && idle {
            app.home.show(app.session.revision);
        }
        x = home.right() + 8.0;
        app_bar_separator(ui, r, x, "brand");
        x += 16.0;
        // While a file loads the design waits: no menus or commands from the bar.
        // The File menu: a text button (New, Open, Save, Save As, Export…). Its handle keeps the
        // old `appbar:folder` name too, for scripts written against the folder button.
        let file = {
            let label = app.tr("ui.file").to_string();
            let galley = ui.painter().layout_no_wrap(label.clone(), FontId::proportional(t.font_button), t.app_bar_text);
            let br = Rect::from_min_size(pos2(x - 13.0, r.center().y - 13.0), vec2(galley.size().x + 16.0, 26.0));
            x = br.right() + 17.0;
            let resp = ui.interact(br, ui.id().with(("ab", "file")), if idle { Sense::click() } else { Sense::hover() });
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, idle, &label));
            crate::scenario::publish_handle("appbar:file", br.center());
            crate::scenario::publish_target_as("appbar:folder", "appbar:file", br);
            let open = ui.ctx().data(|d| d.get_temp::<bool>(egui::Id::new("sc_file_menu")).unwrap_or(false));
            if idle && (resp.hovered() || resp.has_focus() || open) {
                ui.painter().rect_filled(br, t.radius_sm, t.app_bar_hover);
            }
            let ink = if idle { t.app_bar_text } else { t.app_bar_text.gamma_multiply(0.35) };
            ui.painter().galley(pos2(br.left() + 8.0, br.center().y - galley.size().y / 2.0), galley, ink);
            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("sc_file_menu_x"), br.left()));
            idle && resp.clicked()
        };
        // A disabled button is dimmed and ignores clicks (its tooltip still shows).
        let click = |ui: &mut egui::Ui, at: f32, icon: &str, tip: &str, enabled: bool| -> bool {
            let width = if compact_actions && matches!(icon, "save" | "undo" | "redo") { 22.0 } else { 32.0 };
            let br = Rect::from_center_size(pos2(at, r.center().y), vec2(width, 32.0));
            let resp = ui.interact(br, ui.id().with(("ab", icon)), if enabled { Sense::click() } else { Sense::hover() });
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, tip));
            crate::scenario::publish_target(&format!("appbar:{icon}"), br);
            crate::scenario::publish_target(&format!("titlebar:{icon}"), br);
            if enabled && (resp.hovered() || resp.has_focus()) {
                ui.painter().rect_filled(br.shrink(3.0), t.radius_sm, t.app_bar_hover_button);
            }
            icons::paint_app_bar(ui.painter(), Rect::from_center_size(br.center(), vec2(18.0, 18.0)), icon, if enabled { 1.0 } else { 0.35 });
            let clicked = resp.on_hover_text(tip).clicked();
            enabled && clicked
        };
        let step = if compact_actions { 22.0 } else { 34.0 };
        // Save, Undo and Redo use the same actions as the File menu and shortcuts.
        let shown = design_shown(app) && idle;
        let tip = |id: &str, key: &str| {
            let label = app.tr(id);
            if shown { format!("{label} ({key})") } else { crate::i18n::format("ui.needs_design", &[("command", label)]) }
        };
        let (save_tip, undo_tip, redo_tip) = (tip("ui.save", "Ctrl+S"), tip("ui.undo", "Ctrl+Z"), tip("ui.redo", "Ctrl+Y"));
        if click(ui, x, "save", &save_tip, shown) {
            save(app);
        }
        x += step;
        // Narrow windows retain Save; Undo and Redo remain available from their shortcuts and menus.
        if !compact_actions {
            if click(ui, x, "undo", &undo_tip, shown) {
                FileAction::Undo.run(app);
            }
            x += step;
            if click(ui, x, "redo", &redo_tip, shown) {
                FileAction::Redo.run(app);
            }
            x += step;
        }
        if file {
            app.ui.palette_open = false;
            let ctx = ui.ctx().clone();
            let open = ctx.data(|d| d.get_temp::<bool>(egui::Id::new("sc_file_menu")).unwrap_or(false));
            let pass = ctx.cumulative_pass_nr();
            // The File button toggles the menu. Remember the pass it opened on so the same
            // click isn't also treated as a click outside the menu (which would close it at once).
            ctx.data_mut(|d| {
                d.insert_temp(egui::Id::new("sc_file_menu"), !open);
                d.insert_temp(egui::Id::new("sc_file_menu_opened"), pass);
            });
        }
        // Document tabs follow the file actions; the house opens the start page.
        let captions = if app.custom_titlebar { crate::titlebar::WIDTH } else { 0.0 };
        let separator_x = x - step + if compact_actions { 19.0 } else { 24.0 };
        app_bar_separator(ui, r, separator_x, "documents");
        let tabs_start = separator_x + 10.0;
        let report_text = crate::i18n::tr("ui.report_a_bug");
        let report_font = FontId::proportional(t.font_button);
        let label_width = ui.painter().layout_no_wrap(report_text.to_string(), report_font.clone(), t.app_bar_text).size().x;
        // The version links to its release notes; measured here so Report a bug leaves it room.
        let version =
            ui.painter().layout_no_wrap(format!("v{}", crate::update::current_version()), FontId::proportional(t.font_small), t.app_bar_text);
        let (report, report_label) = report_rect(r, captions, tabs_start, label_width, version.size().x);
        // Assistant, Settings, Help and Report a bug form the right-hand cluster.
        if click(ui, help_x(report), "help", app.tr("ui.help"), true) {
            app.help.menu = !app.help.menu;
            let at = help_x(report);
            ui.ctx().data_mut(|d| {
                d.insert_temp(egui::Id::new("sc_help_opened"), true);
                d.insert_temp(egui::Id::new("sc_help_anchor"), at);
            });
        }
        // A newer release: its Update button stays before the right cluster, in the space up to
        // the bar's own file actions on the left.
        let controls_left = report.left() - CLUSTER_BUTTONS;
        let span = Rect::from_min_max(pos2(x, report.top()), pos2(controls_left - 2.0, report.bottom()));
        let update_labeled = report_label;
        let update = crate::update::button(app, ui, span, update_labeled);
        let tabs_end = update.map_or(controls_left, |u| u.left()) - 8.0;
        // Narrow title bars can fit only part of the tab strip. Its hit regions must not
        // extend into the persistent report and window controls.
        let tabs_rect = Rect::from_min_max(pos2(tabs_start.min(tabs_end), r.top()), pos2(tabs_end, r.bottom()));
        let mut tabs_ui = ui.new_child(egui::UiBuilder::new().max_rect(tabs_rect));
        tabs_ui.set_clip_rect(tabs_rect.intersect(ui.clip_rect()));
        crate::documents::tabs(app, &mut tabs_ui, r, tabs_start, tabs_end);
        let response = ui.interact(report, ui.id().with("sc_report_button"), Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), report_text));
        crate::scenario::publish_target("titlebar:report", report);
        ui.painter().rect_filled(report, t.radius_sm, if response.hovered() || response.has_focus() { t.app_bar_hover_button } else { t.app_bar });
        ui.painter().rect_stroke(report, t.radius_sm, Stroke::new(1.0, t.app_bar_separator), egui::StrokeKind::Inside);
        let icon = Rect::from_center_size(pos2(report.left() + 14.0, report.center().y), vec2(18.0, 18.0));
        icons::paint_app_bar(ui.painter(), icon, "bug_report", 1.0);
        if report_label {
            ui.painter().text(pos2(report.left() + REPORT_TEXT_X, report.center().y), Align2::LEFT_CENTER, report_text, report_font, t.app_bar_text);
        }
        if response.on_hover_text(crate::i18n::tr_label("Report a bug — review a screenshot and open a GitHub draft")).clicked() {
            crate::help::run(app, &ui.ctx().clone(), "report");
        }
        // Settings sits immediately left of Help (also Ctrl+,).
        let gear = Rect::from_center_size(pos2(report.left() - 52.0, r.center().y), vec2(32.0, 32.0));
        let gresp = ui.interact(gear, ui.id().with("sc_prefs_button"), Sense::click());
        if gresp.hovered() || gresp.has_focus() {
            ui.painter().rect_filled(gear.shrink(3.0), t.radius_sm, t.app_bar_hover_button);
        }
        icons::paint_app_bar(ui.painter(), gear.shrink(7.0), "settings", 1.0);
        crate::scenario::publish_rect("titlebar:preferences", gear);
        if crate::agent_chat::app_bar_button_shown(app) {
            assistant_button(app, ui, Rect::from_center_size(pos2(report.left() - 86.0, r.center().y), vec2(32.0, 32.0)));
        }
        crate::scenario::publish_handle("titlebar:preferences", gear.center());
        let prefs_tip = if cfg!(target_os = "macos") {
            format!("{} (Cmd+,)", app.tr("ui.preferences"))
        } else {
            format!("{} (Ctrl+,)", app.tr("ui.preferences"))
        };
        if gresp.on_hover_text(prefs_tip).clicked() {
            crate::prefs::open_at(app, 0);
        }
        if version_room(r, captions, tabs_start, version.size().x).is_some() {
            let galley = version;
            let vrect = Align2::RIGHT_CENTER.anchor_size(pos2(r.right() - 12.0 - captions, r.center().y), galley.size());
            let vresp = ui.interact(
                vrect.expand2(vec2(2.0, 4.0)).intersect(Rect::everything_right_of(report.right())),
                ui.id().with("sc_version_link"),
                Sense::click(),
            );
            let url = crate::update::release_notes_url(crate::update::current_version(), crate::update::release_build());
            let vresp = vresp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(&url);
            if vresp.hovered() || vresp.has_focus() {
                ui.painter().hline(vrect.x_range(), vrect.bottom(), Stroke::new(1.0, t.app_bar_text));
            }
            ui.painter().galley(vrect.min, galley, t.app_bar_text);
            crate::scenario::publish_handle("titlebar:version", vrect.center());
            if vresp.clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
        }
        if app.custom_titlebar {
            crate::titlebar::caption_buttons(app, ui, r);
        }
    });
    file_menu(app, ui.ctx());
    let help_x = ui.ctx().data(|d| d.get_temp::<f32>(egui::Id::new("sc_help_anchor")).unwrap_or(300.0));
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
        !app.file_picker_busy() && !app.file_load.busy() && !behind_report && (!needs_design || design_shown(app)) && available
    }

    /// Whether the native menu offers this action: while a widget has keyboard focus (a report's
    /// text field too), its Undo and Redo go to that widget instead of the design.
    pub fn menu_enabled(&self, app: &SolveApp, keyboard_focus: bool) -> bool {
        self.enabled(app) || (matches!(self, Self::Undo | Self::Redo) && keyboard_focus && !app.file_picker_busy() && !app.file_load.busy())
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
    let id = egui::Id::new("sc_file_menu");
    if !ctx.data(|d| d.get_temp::<bool>(id).unwrap_or(false)) {
        return;
    }
    let mut close = false;
    let resp = Container::new(ctx, Surface::Popup)
        .area(egui::Id::new("sc_file_area"))
        .fixed_pos(pos2(ctx.data(|d| d.get_temp::<f32>(egui::Id::new("sc_file_menu_x"))).unwrap_or(100.0), APP_BAR_HEIGHT))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            crate::ui_kit::Menu::show_frame(ui, crate::ui_kit::Menu::frame(), |ui| {
                ui.set_min_width(190.0);
                ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
                ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::NONE;
                ui.visuals_mut().widgets.hovered.weak_bg_fill = Tokens::get().hover;
                let item_if = |ui: &mut egui::Ui, label: &str, key: &str, icon: &str, enabled: bool| {
                    crate::context_menu::list_row(ui, &crate::context_menu::Item::action(label, label, icon).key(key).on_if(enabled)).is_some()
                };
                let item = |ui: &mut egui::Ui, label: &str, key: &str, icon: &str| item_if(ui, label, key, icon, true);
                // Insert, Save and Export act on the design, which Home hides.
                let shown = design_shown(app);
                if item(ui, app.tr("ui.new_design"), "Ctrl+N", "new") {
                    FileAction::New.run(app);
                    close = true;
                }
                if item(ui, &format!("{}…", app.tr("ui.open")), "Ctrl+O", "open") {
                    FileAction::Open.run(app);
                    close = true;
                }
                crate::ui_kit::Menu::button(ui, app.tr("ui.open_recent"), |ui| {
                    for path in app.home.recent.clone() {
                        let missing = !crate::home::available(app, &path);
                        let label = format!("{path}{}", if missing { " (missing)" } else { "" });
                        if crate::context_menu::button(ui, &label, "open").clicked() {
                            FileAction::OpenRecent(path).run(app);
                            close = true;
                            ui.close();
                        }
                    }
                    ui.separator();
                    if crate::context_menu::button(ui, app.tr("ui.clear_recent"), "delete").clicked() {
                        FileAction::ClearRecent.run(app);
                        close = true;
                        ui.close();
                    }
                });
                if item_if(ui, "Insert STEP…", "", "import", shown) {
                    FileAction::Command("file.insert_step").run(app);
                    close = true;
                }
                if item_if(ui, "Insert Mesh…", "", "import", shown) {
                    FileAction::Command("file.insert_mesh").run(app);
                    close = true;
                }
                if item_if(ui, &format!("{}…", app.tr_label("Insert Design")), "", "component", shown) {
                    FileAction::Command("component.insert_design").run(app);
                    close = true;
                }
                if item_if(ui, app.tr("ui.save"), "Ctrl+S", "save", shown) {
                    FileAction::Save.run(app);
                    close = true;
                }
                // Save As shows the shortcut of the command it runs, from the app's own registry.
                let save_as_key = crate::keymap::effective(app, "file.save_as").unwrap_or_default();
                if item_if(ui, &format!("{}…", app.tr("ui.save_as")), &save_as_key, "save", shown) {
                    FileAction::SaveAs.run(app);
                    close = true;
                }
                if item(ui, "Save As JSON…", "", "save") {
                    FileAction::SaveJson.run(app);
                    close = true;
                }
                // The browser build: a copy as a downloaded file, whatever Save does.
                if app.services.download_design.is_some() && item_if(ui, "Download design", "", "export", shown) {
                    download_design(app);
                    close = true;
                }
                if app.services.browser_files.is_some() && item(ui, "Files in this browser…", "", "open") {
                    if let Some(show) = &app.services.browser_files {
                        show();
                    }
                    close = true;
                }
                ui.separator();
                if item_if(ui, "Export STL…", "", "mesh_export", shown) {
                    FileAction::ExportMesh("stl").run(app);
                    close = true;
                }
                if item_if(ui, "Export 3MF…", "", "mesh_export", shown) {
                    FileAction::ExportMesh("3mf").run(app);
                    close = true;
                }
                if item_if(ui, "Send to Slicer", "", "mesh_export", shown && crate::dialogs_save_mesh::available(app)) {
                    app.start("file.send_to_slicer");
                    close = true;
                }
                if item_if(ui, "Export STEP…", "", "export", shown) {
                    FileAction::Export("step").run(app);
                    close = true;
                }
                if item_if(ui, app.tr_label("Export Current View as DXF…"), "", "export", shown) {
                    FileAction::Command("file.export_view_dxf").run(app);
                    close = true;
                }
                if item_if(ui, "Export OBJ…", "", "export", shown) {
                    FileAction::Export("obj").run(app);
                    close = true;
                }
                if item_if(ui, "Export URDF…", "", "export", shown) {
                    FileAction::Export("urdf").run(app);
                    close = true;
                }
                if item(ui, "Export Design as JSON…", "", "export") {
                    FileAction::Export("json").run(app);
                    close = true;
                }
                ui.separator();
                if item_if(ui, &format!("{}…", app.tr_label("Design Properties")), "", "settings", shown) {
                    FileAction::Command("document.metadata").run(app);
                    close = true;
                }
                ui.separator();
                // The browser can install SolidCraft as an app (`install`): only while it offers to.
                if let Some(offer) = crate::install::offered(app)
                    && crate::context_menu::button(ui, crate::i18n::tr("install.menu"), "new")
                        .on_hover_text(crate::i18n::tr("install.button_tip"))
                        .clicked()
                {
                    crate::install::start(app, offer);
                    close = true;
                }
                if item(ui, &format!("{}…", app.tr("ui.preferences")), "", "settings") {
                    FileAction::Settings.run(app);
                    close = true;
                }
                if item(ui, &format!("{}…", app.tr("ui.keyboard_shortcuts")), "", "list") {
                    FileAction::Shortcuts.run(app);
                    close = true;
                }
                if item(ui, app.tr("ui.quit"), "", "close") {
                    FileAction::Quit.run(app);
                    close = true;
                }
            });
        });
    let just_opened = ctx.data(|d| d.get_temp::<u64>(egui::Id::new("sc_file_menu_opened"))) == Some(ctx.cumulative_pass_nr());
    if close || (!just_opened && resp.response.clicked_elsewhere()) {
        ctx.data_mut(|d| d.insert_temp(id, false));
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
    let name = crate::i18n::format("document.copy_name", &[("name", &app.session.doc.name)]);
    save_as_named(app, &name, solidcraft_engine::io::DESIGN_EXT);
}

fn save_as_with_extension(app: &mut SolveApp, extension: &str) {
    let name = app.session.doc.name.clone();
    save_as_named(app, &name, extension);
}

fn save_as_named(app: &mut SolveApp, name: &str, extension: &str) {
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
fn download_design(app: &mut SolveApp) {
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

/// Export to a file with extension `ext`, in `format` when the extension alone doesn't name it.
fn export_as(app: &mut SolveApp, ext: &str, format: Option<&str>) {
    let name = format!("{}.{ext}", app.session.doc.name);
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

/// Choose at most two lines at spaces or after `/`, then truncate each line at
/// Unicode character boundaries. An oversized word stays on one line with an ellipsis.
fn ribbon_button_lines(painter: &egui::Painter, label: &str, width: f32) -> Vec<String> {
    let mut lines = ribbon_label_lines(painter, label, width);
    // Right-to-left labels come in display order (see `bidi`): a line reads from its right end,
    // so a shortened line loses characters on the left, where the ellipsis then goes.
    let rtl = crate::bidi::has_rtl(label);
    for line in &mut lines {
        if ribbon_label_width(painter, line) > width {
            while !line.is_empty() {
                if rtl {
                    line.remove(0);
                } else {
                    line.pop();
                }
                let shortened = if rtl { format!("…{}", line.trim_start()) } else { format!("{}…", line.trim_end()) };
                if ribbon_label_width(painter, &shortened) <= width {
                    break;
                }
            }
            *line = if rtl { format!("…{}", line.trim_start()) } else { format!("{}…", line.trim_end()) };
        }
    }
    lines
}

fn ribbon_label_width(painter: &egui::Painter, text: &str) -> f32 {
    let mut job =
        egui::text::LayoutJob::simple(text.to_owned(), FontId::proportional(crate::theme::Tokens::get().font_body), Color32::WHITE, f32::INFINITY);
    job.halign = egui::Align::Center;
    painter.layout_job(job).size().x
}

fn ribbon_button_width(painter: &egui::Painter, label: &str) -> f32 {
    let widest = ribbon_label_lines(painter, label, BUTTON_MAX_WIDTH - BUTTON_LABEL_PADDING)
        .iter()
        .map(|line| ribbon_label_width(painter, line))
        .fold(0.0_f32, f32::max);
    (widest + BUTTON_LABEL_PADDING).clamp(BUTTON_MIN_WIDTH, BUTTON_MAX_WIDTH)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CjkScript {
    Han,
    Hiragana,
    Katakana,
    Other,
}

fn cjk_script(c: char) -> CjkScript {
    match c {
        '\u{3040}'..='\u{309F}' => CjkScript::Hiragana,
        '\u{30A0}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' | '\u{FF65}'..='\u{FF9F}' => CjkScript::Katakana,
        '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}' => CjkScript::Han,
        _ => CjkScript::Other,
    }
}

/// Whether byte offset `end` sits at a word boundary in a spaceless Japanese ribbon label.
fn cjk_word_break(label: &str, end: usize) -> bool {
    let (Some(prev), Some(next)) = (label.get(..end).and_then(|s| s.chars().next_back()), label.get(end..).and_then(|s| s.chars().next())) else {
        return false;
    };
    let (a, b) = (cjk_script(prev), cjk_script(next));
    if a == CjkScript::Katakana && b == CjkScript::Katakana {
        let before = label.get(..end).unwrap_or_default();
        return before.ends_with("サーフェス") || before.ends_with("フラット");
    }
    match (a, b) {
        (CjkScript::Han, CjkScript::Katakana)
        | (CjkScript::Katakana, CjkScript::Han | CjkScript::Hiragana)
        | (CjkScript::Hiragana, CjkScript::Han | CjkScript::Katakana) => true,
        (CjkScript::Han, CjkScript::Hiragana) => next == 'を',
        (CjkScript::Other, CjkScript::Katakana | CjkScript::Hiragana) => prev.is_ascii_alphabetic(),
        (CjkScript::Katakana | CjkScript::Hiragana, CjkScript::Other) => next.is_ascii_alphabetic(),
        _ => false,
    }
}

fn ribbon_label_lines(painter: &egui::Painter, label: &str, width: f32) -> Vec<String> {
    if let Some((first, second)) = label.split_once('\n') {
        let first = first.trim();
        let second = second.trim();
        if !first.is_empty() && !second.is_empty() {
            return vec![first.to_owned(), second.to_owned()];
        }
    }
    let measure = |text: &str| ribbon_label_width(painter, text);
    let normalized = label.split_whitespace().collect::<Vec<_>>().join(" ");
    let label = normalized.as_str();
    if crate::bidi::has_rtl(label) {
        // Display-order right-to-left text reads from its end: the first line takes the words
        // at the end of the string, the second line the words before them.
        let breaks = label.char_indices().filter(|(_, c)| c.is_whitespace() || *c == '/').map(|(at, c)| at + c.len_utf8());
        let mut start = label.len();
        for cut in std::iter::once(0).chain(breaks).collect::<Vec<_>>().into_iter().rev() {
            if cut >= start {
                continue;
            }
            if start < label.len() && measure(label.get(cut..).unwrap_or_default().trim()) > width {
                break;
            }
            start = cut;
        }
        let (rest, first) = label.split_at(start);
        let mut lines = vec![first.trim().to_owned()];
        if !rest.trim().is_empty() {
            lines.push(rest.trim().to_owned());
        }
        return lines;
    }
    let has_delimiters = label.chars().any(|c| c.is_whitespace() || c == '/');
    let mut end = 0;
    for (at, ch) in label.char_indices() {
        let next = at + ch.len_utf8();
        let is_break = next == label.len() || ch.is_whitespace() || ch == '/' || (!has_delimiters && cjk_word_break(label, next));
        if !is_break {
            continue;
        }
        if end > 0 && measure(label.get(..next).unwrap_or_default().trim()) > width {
            break;
        }
        end = next;
    }
    let (first, rest) = label.split_at(end);
    let mut lines = vec![first.trim().to_owned()];
    if !rest.trim().is_empty() {
        lines.push(rest.trim().to_owned());
    }
    lines
}

fn ribbon_button_galley(painter: &egui::Painter, label: &str, width: f32, color: Color32) -> std::sync::Arc<egui::Galley> {
    let lines = ribbon_button_lines(painter, label, width);
    // Explicit newlines only: egui must never split an oversized word.
    let mut job = egui::text::LayoutJob::simple(lines.join("\n"), FontId::proportional(crate::theme::Tokens::get().font_body), color, f32::INFINITY);
    job.halign = egui::Align::Center;
    painter.layout_job(job)
}

/// Ribbon headings use the translated labels, with normal word casing instead of registry caps.
fn ribbon_label(app: &SolveApp, label: &str) -> String {
    let label = app
        .tr_label(crate::workspace::tab_label(label))
        .split_whitespace()
        .map(|word| {
            let lower = word.to_lowercase();
            let mut chars = lower.chars();
            chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ");
    // Partial catalogs may already translate the word-cased name (for example "Model").
    app.tr_label(&label).to_string()
}

fn ribbon_text(ui: &egui::Ui, at: egui::Pos2, text: &str, size: f32, color: Color32, bold: bool) {
    let text = egui::RichText::new(text).size(size).color(color);
    let text = if bold { text.semibold() } else { text };
    let galley = egui::WidgetText::from(text).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Button);
    let origin = at - galley.size() / 2.0;
    let ppp = ui.ctx().pixels_per_point().max(0.5);
    let origin = pos2((origin.x * ppp).round() / ppp, (origin.y * ppp).round() / ppp);
    ui.painter().galley(origin, galley, color);
}

/// SKETCH while editing a sketch and SHEET METAL while a sheet body is active take the contextual
/// tab style; SHEET METAL is an ordinary tab otherwise. WELDMENTS selects itself on a frame member
/// but keeps the ordinary tab look, as MODEL has when selected.
pub(crate) fn contextual_tab(app: &SolveApp, tab: &str) -> bool {
    tab == "SKETCH" || (tab == "SHEET METAL" && crate::workspace::sheet_body(app).is_some())
}

/// How one ribbon tab is painted: its fill, its label colour and an outline
/// (`Color32::TRANSPARENT` for none).
#[derive(Clone, Copy, Debug, PartialEq)]
struct RibbonTabStyle {
    fill: Color32,
    ink: Color32,
    outline: Color32,
}

/// A contextual tab (Sketch while editing, Sheet Metal for a sheet body) is the loudest tab
/// when it is selected: the solid accent pill. Unselected, it looks like any unselected tab
/// with its accent as an outline, so it still marks the context without outshining the
/// selected tab. Ordinary tabs use the soft accent when selected and the hover colour when
/// hovered.
fn ribbon_tab_style(t: &Tokens, tab: &str, contextual: bool, active: bool, hovered: bool) -> RibbonTabStyle {
    let accent = crate::theme::TabAccent::for_tab(t, tab);
    let idle = if hovered { t.hover } else { Color32::TRANSPARENT };
    let (fill, ink, outline) = match (contextual, active) {
        (true, true) => (accent.color, accent.on_fill(t), Color32::TRANSPARENT),
        (true, false) => (idle, t.text, accent.color),
        (false, true) => (accent.soft, accent.ink, Color32::TRANSPARENT),
        (false, false) => (idle, t.text, Color32::TRANSPARENT),
    };
    RibbonTabStyle { fill, ink, outline }
}

fn ribbon_tabs(app: &mut SolveApp, ui: &mut egui::Ui, row: Rect) {
    let t = Tokens::get();
    let tabs = crate::workspace::visible_tabs(app);
    let widths: Vec<_> = tabs
        .iter()
        .map(|tab| {
            ui.painter().layout_no_wrap(ribbon_label(app, tab), crate::fonts::semibold(crate::theme::Tokens::get().font_button), t.text).size().x
                + 24.0
        })
        .collect();
    let tabs_width: f32 = widths.iter().sum::<f32>()
        + tabs.len().saturating_sub(1) as f32 * 2.0
        + tabs.iter().filter(|tab| contextual_tab(app, tab)).count() as f32 * 6.0;
    let search_width = crate::ui_kit::CommandSearch::closed_width(ui.ctx());
    // The first tab and the search field sit the same distance from the card's top, left and right
    // edges, so their outer corners are concentric with the card's corners.
    let floating = Container::for_layout(app.ui.layout, Surface::Panel).floating();
    let inset = RIBBON_TAB_INSET;
    let corner = if floating { t.radius_lg.saturating_sub(inset as u8).max(t.radius_sm / 2) } else { t.control_radius.saturating_sub(1) };
    let render = crate::render_workspace::active(app);
    let action_labels = [app.tr_label("Render").to_string(), app.tr_label("Cancel").to_string()];
    let action_widths = action_labels
        .each_ref()
        .map(|label| ui.painter().layout_no_wrap(label.clone(), crate::fonts::semibold(t.font_button), t.text).size().x + 24.0);
    let action_width = if render { action_widths.iter().sum::<f32>() + 2.0 * t.space_2 } else { 0.0 };
    let search = Rect::from_min_size(
        pos2(row.right() - search_width - inset - action_width, row.top() + inset),
        vec2(search_width, TAB_ROW_HEIGHT - 2.0 * inset),
    );
    if render {
        let mut x = search.right() + t.space_2;
        for ((id, label), width) in ["render.start", "render.cancel"].into_iter().zip(action_labels).zip(action_widths) {
            let rect = Rect::from_min_size(pos2(x, row.top() + inset), vec2(width, search.height()));
            let ready = crate::render_workspace::job_ready(app, id);
            let response = crate::ui_kit::button_at(ui, id, rect, label, crate::ui_kit::Button::Enabled(ready.is_ok()));
            crate::scenario::publish_target(&format!("ribbon:{id}"), response.rect);
            if response.clicked() {
                app.start(id);
            }
            if let Err(reason) = ready {
                response.on_disabled_hover_text(reason);
            }
            x += width + t.space_2;
        }
    }
    // The single editor is drawn on the foreground layer by shortcut_box::show, so it
    // can grow over the ribbon without being clipped to the tab row.
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("sc_ribbon_search"), search));
    let changed = ui.ctx().data_mut(|d| {
        let id = egui::Id::new("sc_ribbon_visible_tab");
        let state = (app.ui.tab.clone(), row.width());
        let changed = d.get_temp::<(String, f32)>(id) != Some(state.clone());
        d.insert_temp(id, state);
        changed
    });
    let span = Rect::from_min_max(row.left_top() + vec2(inset, 0.0), pos2((search.left() - 6.0).max(row.left() + inset), row.bottom()));
    ui.scope_builder(egui::UiBuilder::new().max_rect(span), |ui| {
        ui.set_clip_rect(span);
        egui::ScrollArea::horizontal()
            .id_salt("ribbon_tabs")
            .animated(false)
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                let (all, _) = ui.allocate_exact_size(vec2(tabs_width, TAB_ROW_HEIGHT), Sense::hover());
                let mut x = all.left();
                for (tab, width) in tabs.into_iter().zip(widths) {
                    let contextual = contextual_tab(app, tab);
                    if contextual {
                        x += 6.0;
                    }
                    let tr = Rect::from_min_size(pos2(x, all.top() + inset), vec2(width, TAB_ROW_HEIGHT - 2.0 * inset));
                    let active = app.ui.tab == tab;
                    if active && changed {
                        ui.scroll_to_rect(tr, Some(egui::Align::Center));
                    }
                    let response = ui.interact(tr, ui.id().with(("tab", tab)), Sense::click());
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, ribbon_label(app, tab)));
                    let style = ribbon_tab_style(&t, tab, contextual, active, response.hovered());
                    ui.painter().rect(tr, corner, style.fill, Stroke::new(1.0, style.outline), egui::StrokeKind::Inside);
                    ribbon_text(ui, tr.center(), &ribbon_label(app, tab), t.font_button, style.ink, active || contextual);
                    if ui.clip_rect().contains(tr.center()) {
                        crate::scenario::publish_target(&format!("tab:{tab}"), tr);
                        crate::scenario::publish_rect(&format!("toolbar_tab:{tab}"), tr);
                        let aliases: &[&str] = match tab {
                            "MODEL" => &["SOLID", "PLASTIC"],
                            "SURFACE" => &["MESH"],
                            "MANAGE" => &["UTILITIES"],
                            _ => &[],
                        };
                        for alias in aliases {
                            crate::scenario::publish_handle(&format!("tab:{alias}"), tr.center());
                        }
                    }
                    if response.clicked() {
                        app.ui.tab = tab.to_string();
                    }
                    x += width + 2.0;
                }
            });
    });
    if matches!(app.ui.tab.as_str(), "SKETCH" | "SHEET METAL") {
        ui.painter().line_segment(
            [row.left_bottom() - vec2(0.0, 1.0), row.right_bottom() - vec2(0.0, 1.0)],
            Stroke::new(2.0, panel_accent(&t, &app.ui.tab, "")),
        );
    } else {
        t.secondary_line(
            ui.painter(),
            [row.left_bottom() - vec2(0.0, 0.5), row.right_bottom() - vec2(0.0, 0.5)],
            t.toolbar.lerp_to_gamma(t.border, 0.5),
        );
    }
}

/// The dropdown chevron every ribbon menu shares.
fn dropdown_arrow(painter: &egui::Painter, at: egui::Pos2, ink: Color32) {
    crate::ui_kit::dropdown_indicator(painter, Rect::from_center_size(at, vec2(10.0, 10.0)), ink);
}

pub(crate) fn shell_layout(app: &SolveApp) -> crate::shell::LayoutMode {
    if crate::render_workspace::active(app) { crate::shell::LayoutMode::Floating } else { app.ui.layout }
}

pub(crate) fn shell_cards(app: &SolveApp, available: Rect) -> crate::shell::Cards {
    if crate::render_workspace::active(app) {
        crate::ui_kit::StageBar::cards(available)
    } else {
        crate::shell::Cards::new(available, shell_layout(app), ribbon_height(app.preferences.ribbon_labels))
    }
}

pub fn toolbar(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    crate::workspace::sync_context(app);
    let layout = shell_layout(app);
    let floating = Container::for_layout(layout, Surface::Panel).floating();
    let labels = app.preferences.ribbon_labels;
    let button_height = if labels { BUTTON_HEIGHT } else { ICON_BUTTON_HEIGHT };
    let group_height = if labels { GROUP_HEIGHT } else { ICON_GROUP_HEIGHT };
    let ribbon_height = ribbon_height(labels);
    let gallery_width = if labels { CONSTRAINT_GRID_WIDTH } else { 6.0 * 27.0 };
    let cards = shell_cards(app, ui.available_rect_before_wrap());
    let height = if floating { cards.bottom - ui.available_rect_before_wrap().top() } else { ribbon_height };
    let fill = if floating { Color32::TRANSPARENT } else { t.toolbar };
    let host = Container::for_layout(layout, Surface::Chrome);
    let ribbon = host.edge("sc_toolbar", Edge::Top);
    let frame = host.edge_frame(fill);
    let chrome = ribbon.exact_size(height).show_separator_line(false).frame(frame).show(ui, |ui| {
        let r = if floating { Rect::from_min_max(cards.workspace.min, cards.ribbon.max) } else { frame.outer_rect(ui.max_rect()) };
        let ws = if floating { cards.workspace } else { Rect::from_min_size(r.left_top(), vec2(WORKSPACE_WIDTH, ribbon_height - 1.0)) };
        let ribbon_left = if floating { cards.ribbon.left() } else { ws.right() };
        if floating {
            for card in [ws, cards.ribbon] {
                let fill =
                    if crate::render_workspace::active(app) && card == ws { crate::theme::TabAccent::current(ui.ctx()).soft } else { t.toolbar };
                let frame = host.frame(ui.ctx(), fill, crate::frame::Side::Left);
                ui.painter().add(frame.paint(card));
                // Blank card space owns clicks as well as its buttons.
                ui.interact(card, ui.id().with(("shell_card", card.left().to_bits())), Sense::click_and_drag());
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
        let bg = ui.interact(if floating { cards.ribbon } else { r }, ui.id().with("toolbar_bg"), Sense::click());
        if bg.secondary_clicked() {
            crate::context_menu::open_for(app, bg.interact_pointer_pos().unwrap_or(r.left_bottom()), crate::context_menu::Target::Toolbar);
        }
        let wresp = ui.interact(ws, ui.id().with("workspace_switcher"), Sense::click());
        let open = app.viewport.context_menu.is_some() && app.menu.target == crate::context_menu::Target::Workspace;
        if wresp.hovered() || open {
            painter.rect_filled(ws, if floating { t.radius_lg } else { 0 }, t.hover);
        }
        // Keep the accent straight while leaving the floating card's top corners round.
        let accent_inset = if crate::render_workspace::active(app) {
            t.space_12
        } else if floating {
            f32::from(t.radius_lg)
        } else {
            0.0
        };
        painter.rect_filled(
            Rect::from_min_size(ws.left_top() + vec2(accent_inset, 0.0), vec2((ws.width() - 2.0 * accent_inset).max(0.0), t.space_3)),
            egui::CornerRadius { nw: 0, ne: 0, sw: if floating { t.radius_sm / 2 } else { 0 }, se: if floating { t.radius_sm / 2 } else { 0 } },
            if crate::render_workspace::active(app) { crate::theme::TabAccent::current(ui.ctx()).color } else { t.accent },
        );
        if !floating {
            t.secondary_line(&painter, [ws.right_top(), ws.right_bottom()], t.border);
        }
        let render = crate::render_workspace::active(app);
        if render {
            crate::ui_kit::StageBar::workspace(ui, ws, "ws_render", app.tr("ui.render"), app.tr("ui.ribbon_workspace"));
        } else {
            let workspace_icon = if render { "ws_render" } else { "ws_design" };
            icons::paint_icon(
                &painter,
                Rect::from_center_size(pos2(ws.center().x, ws.top() + if labels { 39.0 } else { 25.0 }), vec2(30.0, 30.0)),
                workspace_icon,
                1.0,
            );
            ribbon_text(
                ui,
                pos2(ws.center().x, ws.top() + if labels { 69.0 } else { 50.0 }),
                app.tr(if render { "ui.render" } else { "ui.design" }),
                t.font_button,
                t.text,
                true,
            );
            let label = app.tr("ui.ribbon_workspace");
            let label_width =
                painter.layout_no_wrap(label.to_string(), FontId::proportional(crate::theme::Tokens::get().font_small), t.text_dim).size().x;
            ribbon_text(ui, pos2(ws.center().x - 6.0, ws.top() + if labels { 89.0 } else { 70.0 }), label, t.font_small, t.text_dim, false);
            dropdown_arrow(&painter, pos2(ws.center().x - 6.0 + label_width / 2.0 + 7.0, ws.top() + if labels { 89.5 } else { 70.5 }), t.text_dim);
        }
        if wresp.on_hover_text(app.tr("ui.change_workspace")).clicked() {
            crate::context_menu::toggle_for(app, ws, ws.left_bottom(), crate::context_menu::Target::Workspace);
        }
        crate::scenario::publish_rect("toolbar", r);
        crate::scenario::publish_rect("workspace_switcher", ws);
        crate::scenario::publish_target("ribbon:workspace", ws);
        if render {
            crate::render_ui::stage_bar(app, ui, cards.ribbon);
            return;
        }
        ribbon_tabs(app, ui, Rect::from_min_max(pos2(ribbon_left, r.top()), pos2(r.right(), r.top() + TAB_ROW_HEIGHT)));
        // Panels.
        let tab = app.ui.tab.clone();
        // The command whose tool or dialog is open right now, shown pressed in the toolbar.
        let running: Option<String> = match (&app.tool, &app.dialog) {
            (Some(tool), _) => Some(tool.cmd.to_string()),
            // Create Sketch never lands in last_command (it is not repeatable).
            (None, Some(d)) if matches!(d.kind, crate::dialogs::Kind::Sketch) => Some("sketch.create".to_string()),
            (None, Some(_)) => app.last_command.as_ref().map(|(id, _)| id.clone()),
            _ => None,
        };
        let mut px = ribbon_left;
        let top = r.top() + TAB_ROW_HEIGHT;
        let mut drop: Option<(&'static str, &'static str, Option<&'static str>)> = None;
        let mut menu: Option<(egui::Pos2, &'static str)> = None;
        // A drop-down row's pin icon clicked: pin or unpin that command.
        let mut toggle: Option<&'static str> = None;
        // The open drop-down (a button dropped on it leaves the toolbar).
        let mut popup_rect: Option<Rect> = None;
        let mut overflow = Vec::new();
        // Every panel with all its buttons (the user's pins have no cap); a toolbar too narrow
        // for them narrows the buttons, then trims panels into their drop-down and the » menu.
        let lists: Vec<_> = crate::toolbar_custom::panel_lists(&tab)
            .into_iter()
            .map(|(panel, cmds, promote)| {
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
        let sizes: Vec<PanelSize> = lists
            .iter()
            .map(|(panel, _, shown)| {
                let mut widths: Vec<_> = shown
                    .iter()
                    .filter(|c| !app.toolbar_custom.in_gallery(&tab, panel, c.id))
                    .map(|c| {
                        let family = crate::sketch_families::family(c.id)
                            .filter(|f| tab == "SKETCH" && *panel == "CREATE" && f.commands.first().copied() == Some(c.id));
                        let label = family.map_or_else(|| crate::workspace::ribbon_button_label(c), |f| f.label);
                        if labels {
                            ButtonSize { width: ribbon_button_width(&painter, app.tr_label(label)), min_width: BUTTON_MIN_WIDTH }
                        } else {
                            let width = if family.is_some() { 50.0 } else { 40.0 };
                            ButtonSize { width, min_width: width }
                        }
                    })
                    .collect();
                if shown.iter().any(|c| app.toolbar_custom.in_gallery(&tab, panel, c.id)) {
                    widths.push(ButtonSize { width: gallery_width, min_width: gallery_width });
                }
                PanelSize { buttons: widths }
            })
            .collect();
        let right = r.right();
        // The Sketch exit group follows the last sketch panel, and is always kept room for so it
        // stays reachable at every width; when panels go into the » menu it moves to the far right.
        let finish_plan = sizes.last().and_then(|size| if tab == "SKETCH" { Some(Plan::full(size)) } else { None });
        let finish_width = finish_plan.as_ref().map_or(0.0, |p| p.width().max(80.0));
        let normal_count = sizes.len().saturating_sub(usize::from(finish_plan.is_some()));
        let mut plans = fit(sizes.get(..normal_count).unwrap_or_default(), right - 4.0 - px - finish_width);
        let finish_right = plans.iter().any(|p| p.fit == Fit::More);
        plans.extend(finish_plan);
        crate::scenario::publish_count("toolbar_collapsed", plans.iter().filter(|p| p.fit == Fit::Collapsed).count() as f64);
        crate::scenario::publish_count("toolbar_more", plans.iter().filter(|p| p.fit == Fit::More).count() as f64);
        crate::scenario::publish_count(
            "toolbar_trimmed",
            plans.iter().zip(&sizes).filter(|(p, s)| p.fit == Fit::Buttons && p.shown < s.buttons.len()).count() as f64,
        );
        for ((panel, cmds, shown), plan) in lists.into_iter().zip(plans) {
            let panel: &&str = &panel;
            if plan.fit == Fit::More {
                overflow.push((*panel, menu_commands(&cmds, &shown)));
                continue;
            }
            // A panel left with no buttons (all removed from the toolbar, or saved buttons that no
            // longer exist) is drawn collapsed too: one button that opens its drop-down, so its
            // commands never vanish from the toolbar (#7166). Both are COLLAPSED_WIDTH wide.
            let collapsed = plan.fit == Fit::Collapsed || (shown.is_empty() && !cmds.is_empty());
            let trimmed = plan.shown < plan.widths.len();
            let finish = tab == "SKETCH" && *panel == "FINISH SKETCH";
            let pinned = finish;
            let width = if pinned { finish_width } else { plan.width() };
            if pinned && finish_right {
                px = right - finish_width;
                painter.line_segment([pos2(px, top), pos2(px, r.bottom() - 1.0)], t.tertiary_border(t.border));
            }
            let button_widths = &plan.widths;
            let sketch_create = tab == "SKETCH" && *panel == "CREATE";
            let enabled_panel = !cmds.is_empty() || *panel == "SELECT";
            // Per tab: panels of the same name on other tabs (MODIFY, CREATE…) list other commands,
            // and the drop-down's area remembers its size by this id (#874).
            let popup_id = ui.id().with(("panel_menu", &tab, panel));
            // Dropped on the panel's free space (all of a collapsed panel): the button goes to
            // its end.
            let gallery: Vec<_> = shown.iter().copied().filter(|c| app.toolbar_custom.in_gallery(&tab, panel, c.id)).collect();
            let large: Vec<_> = shown.iter().copied().filter(|c| !gallery.iter().any(|g| g.id == c.id)).collect();
            let drawn = if collapsed { 0 } else { plan.shown.min(button_widths.len()) };
            // Buttons narrower than the panel's minimum width (a one-button panel) are centred.
            let buttons_width = button_widths.iter().take(drawn).sum::<f32>() + drawn.saturating_sub(1) as f32 * BUTTON_GAP;
            let inset = if drawn == 0 { 0.0 } else { ((width - 8.0 - buttons_width) / 2.0).max(0.0).floor() };
            let bx = px + 4.0 + inset;
            let free_x = bx + buttons_width + BUTTON_GAP;
            let free = Rect::from_min_size(pos2(free_x, top + 2.0), vec2((px + width - 4.0 - free_x).max(0.0), button_height));
            let fresp = ui.interact(free, ui.id().with(("panel_free", panel)), if collapsed { Sense::click() } else { Sense::hover() });
            let mut opened = false;
            if collapsed {
                // One button: the panel's first command's icon over a caret; it opens the
                // drop-down, which lists every command of the panel.
                let br = Rect::from_center_size(free.center(), vec2((width - 8.0).max(0.0), button_height));
                crate::scenario::publish_handle(&format!("collapsed:{tab}:{panel}"), br.center());
                if fresp.hovered() && enabled_panel {
                    painter.rect_filled(br, t.radius_sm, t.hover);
                }
                if let Some(c) = shown.first().or(cmds.first()) {
                    let info = c.info(&app.session);
                    icons::paint_icon(
                        &painter,
                        Rect::from_center_size(br.center() - vec2(0.0, 3.0), vec2(26.0, 26.0)),
                        info.icon,
                        f32::from(t.icon.a()) / 255.0,
                    );
                }
                dropdown_arrow(&painter, pos2(br.center().x, br.bottom() - 3.0), t.text_dim);
                let tip = format!("{} ({})", app.tr_label(panel), shown.len().max(cmds.len()));
                if fresp.on_hover_text(tip).clicked() && enabled_panel {
                    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("sc_panel_open"), popup_id));
                    opened = true;
                }
            }
            let mut button_x = bx;
            for (c, button_width) in large.iter().zip(button_widths.iter().copied()).take(drawn) {
                // Only the family's default pin follows the part's choice. Explicitly pinned
                // variants remain exact commands, including in customised toolbars.
                let family = crate::sketch_families::family(c.id).filter(|f| sketch_create && f.commands.first().copied() == Some(c.id));
                let selected = family.and_then(|f| app.preferences.sketch_variants.selected(f)).unwrap_or(c);
                let whole = Rect::from_min_size(pos2(button_x, top + 2.0), vec2(button_width, button_height));
                button_x += button_width + BUTTON_GAP;
                let mut br = whole;
                if family.is_some() {
                    if labels {
                        br.max.y = whole.top() + 32.0;
                    } else {
                        br.max.x -= 14.0;
                    }
                }
                let info = crate::render_workspace::interactive_command_info(app, selected);
                let resp = ui.interact(br, ui.id().with(("cmd", c.id)), Sense::click_and_drag());
                crate::scenario::publish_handle(&format!("toolbar:{}", c.id), br.center());
                // The button itself (`toolbar:<id>` is also a drop-down row's while one is open).
                crate::scenario::publish_handle(&format!("button:{}", c.id), br.center());
                crate::scenario::publish_rect(&format!("button:{}", c.id), whole);
                if selected.id != c.id {
                    crate::scenario::publish_handle(&format!("button:{}", selected.id), br.center());
                }
                if resp.drag_started() {
                    resp.dnd_set_drag_payload(ToolbarDrag { id: c.id, from_menu: false });
                }
                if let Some(d) = resp.dnd_hover_payload::<ToolbarDrag>()
                    && d.id != c.id
                {
                    ui_kit::insertion_marker(&painter, br.left() - 1.0, br.top() + 2.0, br.bottom() - 2.0);
                }
                if let Some(d) = resp.dnd_release_payload::<ToolbarDrag>()
                    && d.id != c.id
                {
                    drop = Some((d.id, *panel, Some(c.id)));
                }
                if resp.secondary_clicked() {
                    menu = Some((resp.interact_pointer_pos().unwrap_or(br.left_bottom()), c.id));
                }
                let accent = crate::theme::TabAccent::for_tab(&t, &tab);
                let family_open = family.is_some_and(|f| egui::Popup::is_id_open(ui.ctx(), ui.id().with(("family", f.label)).with("popup")));
                if !finish
                    && (resp.is_pointer_button_down_on()
                        || running.as_deref() == Some(selected.id)
                        || family_open
                        || (selected.id == "view.shots_strip" && app.preferences.render_shots_strip))
                {
                    painter.rect(whole, t.radius_sm, accent.soft, Stroke::new(1.0, accent.color), egui::StrokeKind::Inside);
                } else if resp.hovered() && info.enabled {
                    painter.rect_filled(whole, t.radius_sm, t.hover);
                }
                let icon = if selected.id == "sketch.finish" { "finish" } else { info.icon };
                let side = if finish { 30.0 } else { 26.0 };
                let icon_rect =
                    Rect::from_center_size(pos2(br.center().x, whole.top() + if labels { 16.0 } else { button_height / 2.0 }), vec2(side, side));
                icons::paint_icon(&painter, icon_rect, icon, if info.enabled { 1.0 } else { 0.35 });
                let tr_label = app.tr_label(info.label).to_string();
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled() && info.enabled, &tr_label));
                if ribbon_tooltip(app, resp, selected, info.disabled_reason.as_deref()).clicked() && info.enabled {
                    let id = selected.id;
                    app.start_pinned(id, Some(whole));
                }
                if let Some(f) = family {
                    let ar = if labels {
                        Rect::from_min_max(pos2(whole.left(), whole.top() + 30.0), pos2(whole.right(), whole.top() + 48.0))
                    } else {
                        Rect::from_min_max(pos2(br.right(), whole.top()), whole.right_bottom())
                    };
                    let arrow = ui.interact(ar, ui.id().with(("family", f.label)), Sense::click());
                    crate::scenario::publish_handle(&format!("family:{}", f.label), ar.center());
                    if arrow.hovered() {
                        painter.rect_filled(ar, t.radius_sm, t.hover);
                    }
                    // The title sits on the same line as the other buttons' labels.
                    if labels {
                        let title_y = whole.top() + 39.0;
                        let galley = ribbon_button_galley(&painter, app.tr_label(f.label), ar.width() - 16.0, t.text);
                        painter.galley(pos2(ar.center().x - 4.0, title_y - galley.size().y / 2.0), galley, t.text);
                        dropdown_arrow(&painter, pos2(ar.right() - 6.0, title_y + 0.5), t.text_dim);
                    } else {
                        dropdown_arrow(&painter, ar.center(), t.text_dim);
                    }
                    arrow.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, app.tr_label(f.label)));
                    Container::new(ui.ctx(), Surface::Popup).menu(&arrow).show(|ui| {
                        let variants: Vec<_> = f.commands.iter().filter_map(|id| solidcraft_engine::find_command(id)).collect();
                        dropdown_keys(app, ui, &variants);
                        sketch_menu_rows(app, ui, &variants, &mut menu, &mut toggle);
                    });
                } else if labels {
                    let button_label = crate::workspace::ribbon_button_label(selected);
                    let label = app.tr_label(if button_label == selected.label { info.label } else { button_label });
                    let color = if info.enabled { t.text } else { t.text_dim };
                    let galley = ribbon_button_galley(&painter, label, whole.width() - BUTTON_LABEL_PADDING, color);
                    painter.galley(pos2(whole.center().x, whole.top() + 32.0), galley, t.text);
                }
            }
            if !collapsed && drawn > large.len() {
                if !gallery.is_empty() {
                    // A quiet rule sets the grid apart from the large button before it.
                    t.secondary_line(
                        &painter,
                        [pos2(button_x - 1.5, top + 8.0), pos2(button_x - 1.5, top + button_height.min(46.0))],
                        t.toolbar.lerp_to_gamma(t.border, 0.5),
                    );
                }
                for (index, c) in gallery.iter().enumerate() {
                    let columns = if labels { 3 } else { 6 };
                    let cell = Rect::from_min_size(
                        pos2(button_x + (index % columns) as f32 * 27.0, top + 6.0 + (index / columns) as f32 * 23.0),
                        vec2(24.0, 20.0),
                    );
                    let info = crate::render_workspace::interactive_command_info(app, c);
                    let resp = ui.interact(cell, ui.id().with(("cmd", c.id)), Sense::click_and_drag());
                    let active = running.as_deref() == Some(c.id);
                    let accent = crate::theme::TabAccent::for_tab(&t, &tab);
                    painter.rect(
                        cell,
                        t.radius_sm,
                        if active {
                            if accent.color == t.accent { t.panel.lerp_to_gamma(command_accent(c.icon), 0.18) } else { accent.soft }
                        } else if resp.hovered() && info.enabled {
                            t.hover
                        } else {
                            t.panel
                        },
                        Stroke::new(
                            1.0,
                            if active { if accent.color == t.accent { command_accent(c.icon) } else { accent.color } } else { t.border },
                        ),
                        egui::StrokeKind::Inside,
                    );
                    icons::paint_icon(
                        &painter,
                        Rect::from_center_size(cell.center(), vec2(15.0, 15.0)),
                        c.icon,
                        if info.enabled { 1.0 } else { 0.35 },
                    );
                    for prefix in ["toolbar", "button", "ribbon:constraint"] {
                        crate::scenario::publish_target(&format!("{prefix}:{}", c.id), cell);
                    }
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, info.enabled, app.tr_label(c.label)));
                    if resp.drag_started() {
                        resp.dnd_set_drag_payload(ToolbarDrag { id: c.id, from_menu: false });
                    }
                    if let Some(d) = resp.dnd_release_payload::<ToolbarDrag>()
                        && d.id != c.id
                    {
                        drop = Some((d.id, *panel, Some(c.id)));
                    }
                    if resp.secondary_clicked() {
                        menu = Some((resp.interact_pointer_pos().unwrap_or(cell.left_bottom()), c.id));
                    }
                    if ribbon_tooltip(app, resp, c, info.disabled_reason.as_deref()).clicked() && info.enabled {
                        app.start_at(c.id, Some(cell));
                    }
                }
            }
            // A 20 px label row is also the section's existing command-list button.
            // The group area ends 1 px above the card's bottom edge, so the highlight is
            // RIBBON_TAB_INSET from the card's bottom edge (and from its left edge in the first
            // section): its corner is concentric with the card's.
            let lr = Rect::from_min_size(
                pos2(px + RIBBON_TAB_INSET, top + group_height - SECTION_HEIGHT - (RIBBON_TAB_INSET - 1.0)),
                vec2((width - 2.0 * RIBBON_TAB_INSET).max(0.0), SECTION_HEIGHT),
            );
            let lresp = ui.interact(lr, ui.id().with(("panel", panel)), Sense::click());
            crate::scenario::publish_handle(&format!("panel:{tab}:{panel}"), lr.center());
            crate::scenario::publish_target(&format!("ribbon:section:{tab}:{panel}"), lr);
            let panel_open = ui.ctx().data(|d| d.get_temp::<egui::Id>(egui::Id::new("sc_panel_open"))) == Some(popup_id);
            let accent = crate::theme::TabAccent::for_tab(&t, &tab);
            let hot = lresp.hovered() && enabled_panel;
            if panel_open || hot {
                painter.rect_filled(lr, t.radius_sm, if panel_open { accent.soft } else { t.hover });
            }
            let label = if finish { app.tr("ui.ribbon_exit").to_string() } else { ribbon_label(app, panel) };
            let font = if panel_open || hot {
                crate::fonts::semibold(crate::theme::Tokens::get().font_caption)
            } else {
                FontId::proportional(crate::theme::Tokens::get().font_caption)
            };
            let label_width = painter.layout_no_wrap(label.clone(), font, t.text_dim).size().x;
            let ink = if panel_open { accent.ink } else { t.text_dim };
            ribbon_text(ui, lr.center() - vec2(if finish { 0.0 } else { 6.0 }, 0.0), &label, t.font_caption, ink, panel_open || hot);
            if !finish {
                dropdown_arrow(&painter, pos2(lr.center().x + label_width / 2.0 + 1.0, lr.center().y + 0.5), ink);
            }
            if lresp.clicked() && enabled_panel {
                ui.ctx().data_mut(|d| {
                    let key = egui::Id::new("sc_panel_open");
                    if d.get_temp::<egui::Id>(key) == Some(popup_id) {
                        d.remove::<egui::Id>(key);
                    } else {
                        d.insert_temp(key, popup_id);
                    }
                });
            }
            // Labels, gaps and margins append; actual buttons keep their insert-before target.
            // Respect the current layer so a dropdown covering the panel is not a drop target.
            let panel_rect = Rect::from_min_max(pos2(px, top + 2.0), pos2(px + width, r.bottom()));
            crate::scenario::publish_rect(&format!("panel:{tab}:{panel}"), panel_rect);
            let over_button = ui.input(|input| input.pointer.hover_pos()).is_some_and(|p| {
                let mut x = bx;
                button_widths.iter().take(drawn).any(|width| {
                    let rect = Rect::from_min_size(pos2(x, top + 2.0), vec2(*width, button_height));
                    x += width + BUTTON_GAP;
                    rect.contains(p)
                })
            });
            if !over_button && ui.rect_contains_pointer(panel_rect) && egui::DragAndDrop::payload::<ToolbarDrag>(ui.ctx()).is_some() {
                ui_kit::insertion_marker(&painter, px + width - 4.0, top + 4.0, top + 38.0);
                if ui.input(|input| input.pointer.any_released())
                    && let Some(d) = egui::DragAndDrop::take_payload::<ToolbarDrag>(ui.ctx())
                {
                    drop = Some((d.id, *panel, None));
                }
            }
            let open = ui.ctx().data(|d| d.get_temp::<egui::Id>(egui::Id::new("sc_panel_open"))) == Some(popup_id);
            if open {
                let cmds = if collapsed || trimmed { menu_commands(&cmds, &shown) } else { cmds.clone() };
                let area = Container::new(ui.ctx(), Surface::Popup).area(popup_id).fixed_pos(lr.left_bottom()).order(egui::Order::Foreground).show(
                    ui.ctx(),
                    |ui| {
                        crate::ui_kit::Menu::show_frame(ui, crate::ui_kit::Menu::frame(), |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.set_min_width(200.0);
                            if *panel == "SELECT" {
                                selection_filter(app, ui);
                            } else if cmds.is_empty() {
                                ui.label(egui::RichText::new(crate::i18n::tr_label("Not available yet")).color(t.text_dim));
                            }
                            if sketch_create {
                                sketch_create_menu(app, ui, &cmds, &mut menu, &mut toggle);
                                section_footer(ui);
                                return;
                            }
                            let grouped = crate::workspace::grouped_commands(&tab, panel, &cmds);
                            let cmds: Vec<_> = grouped.iter().map(|(_, c)| *c).collect();
                            let row_w = dropdown_width(app, ui, &cmds);
                            ui.set_width(row_w);
                            // Up and Down move the keyboard focus through the enabled rows (from none
                            // focused: the first or the last), instead of egui's own spatial move.
                            let (down, up) = ui.input(|i| (i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::ArrowUp)));
                            if down || up {
                                let enabled: Vec<&str> = cmds.iter().filter(|c| c.info(&app.session).enabled).map(|c| c.id).collect();
                                let focused = ui.ctx().memory(|m| m.focused());
                                let at = enabled.iter().position(|id| focused == Some(dropdown_row_id(id)));
                                let n = enabled.len();
                                let next = match (at, down) {
                                    (None, true) => Some(0),
                                    (None, false) => n.checked_sub(1),
                                    (Some(i), true) => Some((i + 1) % n.max(1)),
                                    (Some(i), false) => Some((i + n.max(1) - 1) % n.max(1)),
                                };
                                if let Some(id) = next.and_then(|i| enabled.get(i)) {
                                    ui.ctx().memory_mut(|m| {
                                        m.request_focus(dropdown_row_id(id));
                                        m.move_focus(egui::FocusDirection::None);
                                    });
                                }
                            }
                            // The rows scroll when the drop-down would run past the window's bottom.
                            // `min_scrolled_height` lets the rows grow past the size the area kept from
                            // an earlier, shorter list (#874); they still shrink to fit their content.
                            let room = (ui.ctx().content_rect().bottom() - ui.cursor().top() - 64.0).max(ROW_H * 3.0);
                            egui::ScrollArea::vertical().id_salt("panel_menu_rows").max_height(room).min_scrolled_height(room).show(ui, |ui| {
                                for (heading, c) in &grouped {
                                    if let Some(heading) = heading {
                                        section_heading(ui, heading, row_w);
                                    }
                                    let pinned = crate::toolbar_custom::pinned_on(app, &tab, c.id).is_some();
                                    match dropdown_row(app, ui, c, row_w, pinned) {
                                        RowAction::None => {}
                                        RowAction::Menu(at) => menu = Some((at, c.id)),
                                        RowAction::TogglePin => toggle = Some(c.id),
                                        RowAction::Start(rect) => {
                                            app.start_at(c.id, Some(rect));
                                            ui.ctx().data_mut(|d| d.remove::<egui::Id>(egui::Id::new("sc_panel_open")));
                                        }
                                    }
                                }
                            });
                            section_footer(ui);
                        });
                    },
                );
                popup_rect = Some(area.response.rect);
                crate::scenario::publish_rect("toolbar_dropdown", area.response.rect);
                // A drag out of the drop-down ends with a release elsewhere: not a click elsewhere.
                let dragging = egui::DragAndDrop::has_payload_of_type::<ToolbarDrag>(ui.ctx());
                if (area.response.clicked_elsewhere() && !lresp.clicked() && !opened && !dragging) || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    ui.ctx().data_mut(|d| d.remove::<egui::Id>(egui::Id::new("sc_panel_open")));
                }
            }
            px += width;
            painter.line_segment([pos2(px, top + 4.0), pos2(px, top + group_height - 3.0)], t.tertiary_border(t.border));
            px += PANEL_GAP;
        }
        if !overflow.is_empty() {
            let at = Rect::from_min_size(pos2(px.min(right - finish_width - 40.0), top + 4.0), vec2(38.0, button_height));
            ui.scope_builder(egui::UiBuilder::new().max_rect(at), |ui| {
                let response = crate::ui_kit::Menu::button(ui, "»", |ui| {
                    for (panel, cmds) in &overflow {
                        crate::ui_kit::Menu::button(ui, *panel, |ui| {
                            if tab == "SKETCH" && *panel == "CREATE" {
                                sketch_create_menu(app, ui, cmds, &mut menu, &mut toggle);
                                return;
                            }
                            if *panel == "SELECT" {
                                selection_filter(app, ui);
                            }
                            for c in cmds {
                                let info = crate::render_workspace::interactive_command_info(app, c);
                                let response = crate::ui_kit::command_menu_button(
                                    ui,
                                    c.id,
                                    info.label,
                                    info.icon,
                                    &crate::keymap::display_for(app, c.id).unwrap_or_default(),
                                    info.enabled,
                                );
                                crate::scenario::publish_handle(&format!("toolbar:{}", c.id), response.rect.center());
                                if response.clicked() {
                                    app.start_at(c.id, Some(response.rect));
                                    ui.close();
                                }
                            }
                        });
                    }
                });
                crate::scenario::publish_target("toolbar:overflow", response.response.rect);
                response.response.on_hover_text(crate::i18n::tr_label("More toolbar panels"));
            });
        }

        // Dragging a command: its icon follows the pointer; a toolbar button dragged off the
        // toolbar (or into the open drop-down) shows it will be removed.
        let pointer = ui.input(|i| i.pointer.latest_pos());
        let over_popup = |p: egui::Pos2| popup_rect.is_some_and(|pr| pr.contains(p));
        let leaves = |p: egui::Pos2| !r.contains(p) || over_popup(p);
        if let (Some(d), Some(p)) = (egui::DragAndDrop::payload::<ToolbarDrag>(ui.ctx()), pointer) {
            let removing = !d.from_menu && leaves(p);
            drag_ghost(app, ui.ctx(), d.id, p, removing);
            if ui.input(|i| i.pointer.any_released()) && drop.is_none() {
                egui::DragAndDrop::clear_payload(ui.ctx());
                if removing {
                    crate::toolbar_custom::remove(app, &tab, d.id);
                }
            }
        }
        if let Some((id, panel, before)) = drop {
            crate::toolbar_custom::reorder(app, &tab, id, panel, before);
        }
        if let Some(id) = toggle {
            crate::toolbar_custom::toggle(app, &tab, id);
        }
        if let Some((at, id)) = menu {
            ui.ctx().data_mut(|d| d.remove::<egui::Id>(egui::Id::new("sc_panel_open")));
            crate::context_menu::open_for(app, at, crate::context_menu::Target::ToolbarCommand { id: id.to_string() });
        }
    });
    if floating {
        let frame = host.frame(ui.ctx(), t.toolbar, crate::frame::Side::Left);
        for card in [cards.workspace, cards.ribbon] {
            Container::paint_inset(ui.painter(), frame.widget_rect(card), frame);
        }
    } else {
        Container::paint_inset(ui.painter(), chrome.response.rect - frame.outer_margin, frame);
    }
}

/// A short divider between the brand, file actions and document tabs.
fn app_bar_separator(ui: &egui::Ui, bar: Rect, x: f32, group: &str) {
    let rect = Rect::from_center_size(pos2(x, bar.center().y), vec2(1.0, 20.0));
    ui.painter().line_segment(
        [rect.center_top(), rect.center_bottom()],
        Stroke::new(1.0, Tokens::get().app_bar.lerp_to_gamma(Tokens::get().app_bar_separator, 0.4)),
    );
    crate::scenario::publish_rect(&format!("titlebar:separator:{group}"), rect);
}

/// The chat panel's compact entry point. State stays in agent_chat; the bar only paints it.
fn assistant_button(app: &mut SolveApp, ui: &mut egui::Ui, at: Rect) {
    use crate::agent_chat::AssistantMark;
    let resp = ui.interact(at, ui.id().with("assistant_button"), Sense::click());
    crate::scenario::publish_target("titlebar:assistant", at);
    let mark = crate::agent_chat::assistant_mark(app);
    let tip = crate::agent_chat::assistant_tip(mark);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &tip));
    let state = match mark {
        AssistantMark::Quiet => crate::ui_kit::AssistantEntryState::Quiet,
        AssistantMark::Ready => crate::ui_kit::AssistantEntryState::Ready,
        AssistantMark::Busy => crate::ui_kit::AssistantEntryState::Busy,
        AssistantMark::NeedsAuth => crate::ui_kit::AssistantEntryState::NeedsAuth,
        AssistantMark::Stopped => crate::ui_kit::AssistantEntryState::Stopped,
        AssistantMark::Waiting(count) => crate::ui_kit::AssistantEntryState::Waiting(count),
    };
    crate::ui_kit::assistant_entry(ui, &resp, state, app.agent_chat.open, ui.ctx().global_style().animation_time > 0.0);
    if resp.on_hover_text(tip).clicked() {
        crate::agent_chat::toggle(app);
    }
}

/// What a click on a panel drop-down row asks for.
enum RowAction {
    None,
    Start(Rect),
    Menu(egui::Pos2),
    TogglePin,
}

const ROW_H: f32 = 30.0;

const CONSTRAINT_GRID_WIDTH: f32 = 82.0;

/// Registry parameter syntax is not a user-facing description. Reuse existing catalog prose
/// where present; other commands intentionally show only their name and registered shortcut.
fn ribbon_tooltip(app: &SolveApp, response: egui::Response, command: &solidcraft_engine::CommandSpec, disabled: Option<&str>) -> egui::Response {
    response.on_hover_ui(|ui| {
        let t = Tokens::get();
        let name = app.tr_label(command.info(&app.session).label);
        let name_width =
            ui.painter().layout_no_wrap(name.to_string(), crate::fonts::semibold(crate::theme::Tokens::get().font_heading), t.text).size().x;
        let key_width = crate::keymap::display_for(app, command.id)
            .map(|key| {
                ui.painter().layout_no_wrap(key.to_string(), FontId::monospace(crate::theme::Tokens::get().font_body), t.text_dim).size().x + 24.0
            })
            .unwrap_or(0.0);
        ui.set_width(280.0_f32.max(name_width + key_width + 12.0));
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(name_width, 20.0), Sense::hover());
            ribbon_text(ui, rect.center(), name, t.font_heading, t.text, true);
            if let Some(key) = crate::keymap::display_for(app, command.id) {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::Frame::NONE
                        .fill(t.panel_header)
                        .stroke(t.tertiary_border(t.border))
                        .corner_radius(t.radius_sm.saturating_sub(1))
                        .inner_margin(egui::Margin::symmetric(5, 1))
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(key).monospace().color(t.text_dim));
                        });
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

fn section_heading(ui: &mut egui::Ui, label: &str, width: f32) {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 24.0), Sense::hover());
    ui.painter().text(
        rect.left_center() + vec2(6.0, 0.0),
        Align2::LEFT_CENTER,
        crate::i18n::tr_label(label).to_uppercase(),
        crate::fonts::semibold(crate::theme::Tokens::get().font_small),
        t.text_dim,
    );
}

fn section_footer(ui: &mut egui::Ui) {
    ui.separator();
    ui.add_space(4.0);
    ui.add(
        egui::Label::new(
            egui::RichText::new(crate::i18n::tr("ui.ribbon_pin_footer")).size(crate::theme::Tokens::get().font_small).color(Tokens::get().text_dim),
        )
        .wrap(),
    );
    ui.add_space(4.0);
}

/// Stable row id inside and outside the scrolling list, for keyboard focus.
fn dropdown_row_id(id: &str) -> egui::Id {
    egui::Id::new(("dropdown_row", id))
}

/// How wide a panel drop-down's rows are: the widest label and shortcut, at least 200.
fn dropdown_width(app: &SolveApp, ui: &egui::Ui, cmds: &[&'static solidcraft_engine::CommandSpec]) -> f32 {
    let t = Tokens::get();
    let font = FontId::proportional(t.font_button);
    cmds.iter()
        .map(|c| {
            let label = ui.painter().layout_no_wrap(app.tr_label(c.info(&app.session).label).to_string(), font.clone(), t.text).size().x;
            let key = crate::keymap::display_for(app, c.id)
                .map(|k| ui.painter().layout_no_wrap(k.to_string(), FontId::monospace(t.font_button), t.text).size().x + 24.0)
                .unwrap_or(0.0);
            6.0 + 20.0 + 8.0 + label + key + 30.0
        })
        .fold(200.0, f32::max)
}

/// One command row of a panel drop-down: icon, label and shortcut, with the hover background
/// under the pointer or keyboard focus (like every menu).
fn dropdown_row(app: &SolveApp, ui: &mut egui::Ui, c: &'static solidcraft_engine::CommandSpec, width: f32, pinned: bool) -> RowAction {
    let t = Tokens::get();
    let info = crate::render_workspace::interactive_command_info(app, c);
    let (rect, _) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::hover());
    // Every row drags onto the toolbar (to pin it there); enabled rows also start their command.
    let sense = if info.enabled { Sense::click_and_drag() } else { Sense::drag() };
    let resp = ui.interact(rect, dropdown_row_id(c.id), sense);
    // Focus can settle on the following frame, after the key event has gone.
    // Keep the focused row visible even then in a drop-down taller than the window.
    if resp.has_focus() && !ui.clip_rect().contains_rect(rect) {
        resp.scroll_to_me_animation(None, egui::style::ScrollAnimation::none());
    }
    if resp.drag_started() {
        resp.dnd_set_drag_payload(ToolbarDrag { id: c.id, from_menu: true });
    }
    // The pin at the row's end (above the row, so it takes its own clicks).
    let pr = Rect::from_center_size(pos2(rect.right() - 13.0, rect.center().y), vec2(20.0, 20.0));
    let pin = ui.interact(pr, ui.id().with(("dropdown_pin", c.id)), Sense::click());
    let pointer_on = (resp.hovered() || pin.hovered()) && !egui::DragAndDrop::has_any_payload(ui.ctx());
    let label = app.tr_label(info.label).to_string();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled() && info.enabled, &label));
    crate::scenario::publish_handle(&format!("toolbar:{}", c.id), rect.center());
    crate::scenario::publish_handle(&format!("dropdown:{}", c.id), rect.center());
    let hot = info.enabled && (pointer_on || resp.has_focus());
    let mut row = crate::ui_kit::MenuItem::new(c.id, &label, info.icon);
    row.enabled = info.enabled;
    if c.id == "view.shots_strip" {
        row.checked = Some(app.preferences.render_shots_strip);
    }
    row.highlighted = hot;
    let shortcut = crate::keymap::display_for(app, c.id).unwrap_or_default();
    row.shortcut = &shortcut;
    row.paint(ui, rect, &resp, 26.0);
    if hot {
        crate::scenario::publish_handle(&format!("dropdown_hot:{}", c.id), rect.center());
    }
    let p = ui.painter();
    if pointer_on || resp.has_focus() {
        if pin.hovered() {
            p.rect_filled(pr, t.radius_sm.saturating_sub(1), t.border.gamma_multiply(0.6));
        }
        pin_icon(p, pr, pinned, if pinned { t.accent } else { t.text_dim });
        crate::scenario::publish_handle(&format!("dropdown_pin:{}", c.id), pr.center());
    }
    let tip = if pinned { "Remove from Toolbar" } else { "Pin to Toolbar" };
    if pin.on_hover_text(tip).clicked() {
        return RowAction::TogglePin;
    }
    // A disabled row says why; a documented command also shows its help (#2758).
    let reason = info.disabled_reason.as_deref().filter(|_| !info.enabled).map(|reason| app.tr_label(reason));
    let resp = match crate::command_help::row_tip(c.id, &label, Some(shortcut.as_str()).filter(|k| !k.is_empty()), reason) {
        Some(tip) => crate::command_help::tooltip(resp, c.id, tip),
        None => resp,
    };
    if resp.secondary_clicked() {
        return RowAction::Menu(resp.interact_pointer_pos().unwrap_or(rect.left_bottom()));
    }
    if resp.clicked() && info.enabled {
        return RowAction::Start(rect);
    }
    RowAction::None
}

/// Creation menus keep related variants together, including collapsed and overflow panels.
fn sketch_create_menu(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    cmds: &[&'static solidcraft_engine::CommandSpec],
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
                    sketch_menu_rows(app, ui, &variants, menu, toggle);
                });
                crate::scenario::publish_handle(&format!("create_family:{}", f.label), response.response.rect.center());
            }
        }
        ui.separator();
        let rest: Vec<_> = cmds.iter().copied().filter(|c| crate::sketch_families::family(c.id).is_none()).collect();
        dropdown_keys(app, ui, &rest);
        sketch_menu_rows(app, ui, &rest, menu, toggle);
    });
}

/// Family popups use the same ordered arrow-key navigation as a panel's command list.
fn dropdown_keys(app: &SolveApp, ui: &egui::Ui, cmds: &[&'static solidcraft_engine::CommandSpec]) {
    let (down, up) = ui.input(|i| (i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::ArrowUp)));
    if !down && !up {
        return;
    }
    // A submenu renders before the remaining rows of its parent. Handle a key once, but
    // leave the event available for the focused row's scroll-into-view behaviour.
    let handled = egui::Id::new("sketch_menu_key_pass");
    let pass = ui.ctx().cumulative_pass_nr();
    if ui.ctx().data(|d| d.get_temp::<u64>(handled)) == Some(pass) {
        return;
    }
    ui.ctx().data_mut(|d| d.insert_temp(handled, pass));
    let enabled: Vec<_> = cmds.iter().filter(|c| crate::render_workspace::interactive_command_info(app, c).enabled).collect();
    if enabled.is_empty() {
        return;
    }
    let focused = ui.ctx().memory(|m| m.focused());
    let at = enabled.iter().position(|c| focused == Some(dropdown_row_id(c.id)));
    let n = enabled.len();
    let next = match (at, down) {
        (None, true) => 0,
        (None, false) => n - 1,
        (Some(i), true) => (i + 1) % n,
        (Some(i), false) => (i + n - 1) % n,
    };
    if let Some(c) = enabled.get(next) {
        ui.ctx().memory_mut(|m| {
            m.request_focus(dropdown_row_id(c.id));
            m.move_focus(egui::FocusDirection::None);
        });
    }
}

fn sketch_menu_rows(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    cmds: &[&'static solidcraft_engine::CommandSpec],
    menu: &mut Option<(egui::Pos2, &'static str)>,
    toggle: &mut Option<&'static str>,
) {
    let width = dropdown_width(app, ui, cmds);
    for c in cmds {
        let pinned = crate::toolbar_custom::pinned_on(app, "SKETCH", c.id).is_some();
        match dropdown_row(app, ui, c, width, pinned) {
            RowAction::None => {}
            RowAction::Menu(at) => *menu = Some((at, c.id)),
            RowAction::TogglePin => *toggle = Some(c.id),
            RowAction::Start(rect) => {
                app.start_at(c.id, Some(rect));
                ui.ctx().data_mut(|d| d.remove::<egui::Id>(egui::Id::new("sc_panel_open")));
                ui.close();
            }
        }
    }
}

type ToolbarDrag = ui_kit::ReorderPayload<&'static str>;

/// The dragged command's icon under the pointer, crossed out when releasing removes it.
fn drag_ghost(app: &SolveApp, ctx: &egui::Context, id: &str, at: egui::Pos2, removing: bool) {
    let t = Tokens::get();
    let Some(c) = solidcraft_engine::find_command(id) else { return };
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("sc_toolbar_drag")));
    let r = Rect::from_center_size(at + vec2(14.0, 16.0), vec2(30.0, 30.0));
    p.rect(r, t.radius_sm, t.toolbar.gamma_multiply(0.92), Stroke::new(1.0, if removing { t.error } else { t.accent }), egui::StrokeKind::Inside);
    let info = c.info(&app.session);
    icons::paint_icon(&p, r.shrink(5.0), info.icon, f32::from(t.icon.a()) / 255.0);
    if removing {
        let s = Stroke::new(2.0, t.error);
        let x = Rect::from_center_size(r.right_top(), vec2(9.0, 9.0));
        p.circle_filled(x.center(), 7.0, t.toolbar);
        crate::icons::glyphs::cross(&p, x, s);
    }
    crate::scenario::publish_handle(if removing { "toolbar_drag:remove" } else { "toolbar_drag" }, at);
}

/// The SELECT panel: what a click in the view picks.
pub(crate) fn selection_filter(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui.label(egui::RichText::new(crate::i18n::tr_label("Selection filter")).color(t.text_dim).size(t.font_small));
    if crate::ui_kit::checkbox(ui, crate::i18n::tr_label("Select bodies (a face picks its body)"), &mut app.ui.pick_bodies).changed() {
        app.ui.pick_components = false;
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
        if crate::ui_kit::checkbox(ui, crate::i18n::tr_label(label), &mut on).changed() {
            app.ui.pick_off.retain(|x| x != k);
            if !on {
                app.ui.pick_off.push(k.to_string());
            }
        }
    }
}

/// Two Esc presses within this many seconds finish the open sketch.
pub const ESCAPE_TWICE_S: f64 = 0.4;

/// Global keyboard shortcuts (when no text field has focus).
pub fn shortcuts(app: &mut SolveApp, ctx: &egui::Context) {
    if design_shown(app) {
        crate::navigation::step_key_hold(app, ctx);
    } else {
        app.viewport.key_hold = None;
    }
    if app.file_load.busy() {
        return;
    }
    // File is a custom Area, so it must claim Escape before the underlying editors and
    // sketch shortcuts. Clear raw events too: egui may replay them on a layout pass.
    let file_menu_id = egui::Id::new("sc_file_menu");
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
    if crate::params_dialog::cancel_on_escape(app, ctx) {
        return;
    }
    // A dimension value that is taking the keyboard: letters belong to it (a parameter name
    // starting with L or D), even on the frame before its field holds the focus.
    if egui::Popup::is_any_open(ctx)
        || ctx.egui_wants_keyboard_input()
        || crate::dim_view::editing()
        || crate::dialogs_save_mesh::popup_open(ctx)
        || crate::hole_presets::name_prompt_open(ctx, app.dialog.as_ref())
    {
        return;
    }
    let (keys, mods) = ctx.input(|i| {
        let keys: Vec<(egui::Key, bool)> = i
            .events
            .iter()
            .filter_map(|e| if let egui::Event::Key { key, pressed: true, repeat, .. } = e { Some((*key, *repeat)) } else { None })
            .collect();
        (keys, i.modifiers)
    });
    // Home hides the design: only New, Open and Preferences work there.
    let shown = design_shown(app);
    if shown {
        crate::edit_clipboard::handle(app, ctx);
    } else {
        crate::edit_clipboard::flush(app, ctx);
    }
    for (k, repeat) in keys {
        use egui::Key;
        // Only Esc pressed twice in a row finishes a sketch: any other key in between resets it.
        if k != Key::Escape {
            app.last_escape = None;
        }
        if shown && crate::navigation::view_key(app, ctx, k, mods, repeat) {
            continue;
        }
        if mods.command {
            match k {
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
                // End navigation before the underlying tool or dialog; sketch double-Escape
                // handling starts with the next press.
                if app.viewport.nav.take().is_some() {
                    app.last_escape = None;
                    continue;
                }
                // Two quick presses in a sketch finish it (keeping it, like Finish Sketch);
                // the first one has already cancelled whatever was active.
                let now = ctx.input(|i| i.time);
                let again = app.last_escape.is_some_and(|t| now - t <= ESCAPE_TWICE_S);
                app.last_escape = Some(now);
                // Not while a tool is still out (its first Esc only ended a chain): this press
                // puts it down, and the next quick one finishes.
                if again
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
                    if blank && app.session.active_sketch.is_none() && app.run("timeline.delete", json!({ "feature": sketch.to_string() })).is_ok() {
                        app.session.undo.truncate(depth + 1);
                    }
                    continue;
                }
                if crate::sketch_text_move::cancel_drag(app) || crate::occurrence_drag::cancel(app) {
                    continue;
                }
                if crate::select_other::is_open(app) {
                    crate::select_other::close(app);
                } else if app.tool.is_some() {
                    crate::tools::finish(app);
                } else if app.dialog.is_some() || app.pending_feature_edit.is_some() {
                    crate::dialogs::cancel(app);
                } else if !app.session.selection.is_empty() || app.session.active_sketch.is_none() {
                    crate::edit_clipboard::drop_cut(app);
                    app.tree.picked_joints.clear();
                    let _ = app.run("select.clear", json!({}));
                }
                // A single Esc with nothing left to cancel keeps the sketch open (#1804); a quick
                // second press finishes it (above).
            }
            Key::S if !mods.shift && !mods.alt => {
                app.start("ui.palette");
            }
            Key::V => crate::context_menu::toggle_visibility(app),
            Key::F2 => {
                let at = ctx.input(|i| i.pointer.latest_pos()).unwrap_or(ctx.content_rect().center());
                crate::context_menu::rename_selection(app, at);
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
            let menu_open = |h: &crate::scenario::Harness| h.ctx.data(|d| d.get_temp::<bool>(egui::Id::new("sc_file_menu")).unwrap_or(false));
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
        assert!(h.ctx.data(|d| d.get_temp::<bool>(egui::Id::new("sc_file_menu")).unwrap_or(false)));
        h.step(&json!({"key":"Escape"})).unwrap();
        assert!(!h.ctx.data(|d| d.get_temp::<bool>(egui::Id::new("sc_file_menu")).unwrap_or(false)));
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
            assert!(!h.ctx.data(|data| data.get_temp::<bool>(egui::Id::new("sc_file_menu")).unwrap_or(false)));
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
        assert!(h.app.prefs_window.open);
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
        assert!(h.app.dialog.is_none() && h.app.file_picker.is_none() && !h.app.prefs_window.open && !h.app.keymap.open && !h.app.agent_chat.open);
        // A focused report field still gets the menu's Undo and Redo.
        assert!(FileAction::Undo.menu_enabled(&h.app, true) && !FileAction::Undo.menu_enabled(&h.app, false));
        assert!(!FileAction::Save.menu_enabled(&h.app, true));
        // Quit waits as behind any modal window: it would switch to a design with unsaved changes
        // under the report.
        assert!(!FileAction::Quit.enabled(&h.app));
        FileAction::Quit.run(&mut h.app);
        assert!(h.app.docs.quitting.is_none() && h.app.docs.closing.is_none());
        // Help's windows wait too; Report an Issue… shows the report.
        for item in ["about", "shortcuts", "messages", "crashes", "connect"] {
            assert!(!crate::help::available(&h.app, item), "{item}");
            crate::help::run(&mut h.app, &h.ctx, item);
        }
        assert!(!h.app.help.about && !h.app.keymap.open && !h.app.messages.open && h.app.help.connect.is_none());
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

    fn sizes(buttons: &[usize]) -> Vec<PanelSize> {
        buttons.iter().map(|&b| PanelSize { buttons: vec![ButtonSize { width: BUTTON_MIN_WIDTH, min_width: BUTTON_MIN_WIDTH }; b] }).collect()
    }

    fn used_with_more(plans: &[Plan]) -> f32 {
        used(plans) + if plans.iter().any(|p| p.fit == Fit::More) { MORE_ROOM } else { 0.0 }
    }

    #[test]
    fn panels_give_up_one_button_at_a_time_then_move_to_more_and_none_is_dropped() {
        // A panel with 14 buttons and three default ones.
        let panels = sizes(&[14, 4, 2, 2]);
        let full: f32 = [14, 4, 2, 2].iter().map(|&b| panel_width(&vec![BUTTON_MIN_WIDTH; b]) + PANEL_GAP).sum();
        let all = |plans: &[Plan]| plans.iter().map(|p| (p.fit, p.shown)).collect::<Vec<_>>();
        assert_eq!(all(&fit(&panels, full)), [(Fit::Buttons, 14), (Fit::Buttons, 4), (Fit::Buttons, 2), (Fit::Buttons, 2)]);
        // A little short: the widest panel hands one button to its drop-down; the others keep all theirs.
        assert_eq!(all(&fit(&panels, full - 10.0)), [(Fit::Buttons, 13), (Fit::Buttons, 4), (Fit::Buttons, 2), (Fit::Buttons, 2)]);
        // Every panel down to one button (collapsed) fits.
        let collapsed = 4.0 * (COLLAPSED_WIDTH + PANEL_GAP);
        assert_eq!(all(&fit(&panels, collapsed)), [(Fit::Collapsed, 1); 4]);
        // Narrower: the last one goes into the » menu, and the room it frees goes back to buttons.
        let more = fit(&panels, collapsed - 1.0);
        assert_eq!(more.iter().map(|p| p.fit).collect::<Vec<_>>(), [Fit::Collapsed, Fit::Collapsed, Fit::Collapsed, Fit::More]);
        assert!(more.iter().take(3).all(|p| p.shown >= 1));
        let more = fit(&sizes(&[14, 4, 2, 2, 2, 2]), 5.0 * (COLLAPSED_WIDTH + PANEL_GAP) + MORE_ROOM - 1.0);
        assert_eq!(more.last().unwrap().fit, Fit::More);
        assert!(more.iter().any(|p| p.fit == Fit::Buttons && p.shown > 1), "overflow room restores buttons");
        // Then all of them.
        assert!(fit(&panels, 0.0).iter().all(|p| p.fit == Fit::More));
        assert_eq!(fit(&panels, f32::NAN).len(), 4);
        // No cap: every additional button keeps its width and gap.
        assert_eq!(panel_width(&[BUTTON_MIN_WIDTH; 30]) - panel_width(&[BUTTON_MIN_WIDTH; 20]), 10.0 * (BUTTON_MIN_WIDTH + BUTTON_GAP));
    }

    /// #2108: whatever the room, the toolbar uses it. It never leaves more than one button's width
    /// blank while something is hidden, never runs past the room, and shrinks steadily as the
    /// room does (so a few pixels never flip it back and forth).
    #[test]
    fn panels_fill_the_room_they_have_at_every_width() {
        let layouts = [
            // SOLID: CREATE, MODIFY, ASSEMBLE, CONFIGURE, CONSTRUCT, INSPECT, INSERT, SELECT.
            vec![(4, false), (4, false), (3, false), (2, false), (2, false), (2, false), (2, false), (0, false)],
            // SKETCH: labelled CREATE, then MODIFY … FINISH SKETCH.
            vec![(8, true), (2, false), (3, false), (6, false), (2, false), (2, false), (0, false), (1, false)],
            // Many pins on one panel.
            vec![(14, false), (4, false), (2, false), (2, false)],
        ];
        for layout in layouts {
            let panels: Vec<PanelSize> = layout
                .iter()
                .map(|&(buttons, labelled)| PanelSize {
                    buttons: (0..buttons)
                        .map(|i| ButtonSize {
                            width: if labelled { BUTTON_MAX_WIDTH - (i % 3) as f32 * 5.0 } else { BUTTON_MIN_WIDTH },
                            min_width: BUTTON_MIN_WIDTH,
                        })
                        .collect(),
                })
                .collect();
            let mut last: Option<(f32, usize)> = None;
            for room in (0..=2000).rev().map(|r| r as f32) {
                let plans = fit(&panels, room);
                let width = used_with_more(&plans);
                let hidden = plans
                    .iter()
                    .zip(&panels)
                    .any(|(p, s)| p.fit != Fit::Buttons || p.shown < s.buttons.len() || p.widths.iter().zip(&s.buttons).any(|(w, b)| *w < b.width));
                let more = plans.iter().filter(|p| p.fit == Fit::More).count();
                if more < panels.len() {
                    assert!(width <= room + 0.01, "{layout:?} at {room}: {width} overruns");
                }
                // One button: an icon's stride, or a collapsed panel's when the next panel waits in the » menu.
                let button = if more > 0 { COLLAPSED_WIDTH + PANEL_GAP } else { BUTTON_MIN_WIDTH + BUTTON_GAP };
                if hidden && more < panels.len() {
                    assert!(room - width < button, "{layout:?} at {room}: {} px blank while something is hidden", room - width);
                }
                // Fewer pixels never bring a panel back out of the » menu.
                if let Some((_, prev_more)) = last {
                    assert!(more >= prev_more, "{layout:?} at {room}: a panel came back out of the » menu");
                }
                last = Some((width, more));
                // A panel shows its icons: one at the very least.
                for (p, s) in plans.iter().zip(&panels) {
                    if p.fit != Fit::More && !s.buttons.is_empty() {
                        assert!(p.shown >= 1, "{layout:?} at {room}: a panel without icons");
                    }
                }
            }
        }
    }

    #[test]
    fn variable_width_buttons_narrow_before_rightmost_equal_panels_lose_buttons() {
        let panels = vec![PanelSize {
            buttons: vec![
                ButtonSize { width: 76.0, min_width: BUTTON_MIN_WIDTH },
                ButtonSize { width: 63.0, min_width: BUTTON_MIN_WIDTH },
                ButtonSize { width: 54.0, min_width: BUTTON_MIN_WIDTH },
            ],
        }];
        let full = used(&panels.iter().map(Plan::full).collect::<Vec<_>>());
        let narrow = fit(&panels, full - 1.0);
        assert_eq!(narrow[0].shown, 3);
        assert_eq!(narrow[0].widths, [75.0, 62.0, 54.0]);
        let minimum = panel_width(&[BUTTON_MIN_WIDTH; 3]) + PANEL_GAP;
        let narrow = fit(&panels, minimum);
        assert_eq!(narrow[0].shown, 3);
        assert_eq!(narrow[0].widths, [BUTTON_MIN_WIDTH; 3]);
        assert_eq!(fit(&panels, minimum - 1.0)[0].shown, 2);

        let equal = sizes(&[3, 3]);
        let full = used(&equal.iter().map(Plan::full).collect::<Vec<_>>());
        assert_eq!(fit(&equal, full - 1.0).iter().map(|p| p.shown).collect::<Vec<_>>(), [3, 2]);
    }

    #[test]
    fn constraint_gallery_stays_fixed_and_hands_all_cells_to_the_menu_together() {
        let panels = vec![PanelSize {
            buttons: vec![
                ButtonSize { width: BUTTON_MAX_WIDTH, min_width: BUTTON_MIN_WIDTH },
                ButtonSize { width: CONSTRAINT_GRID_WIDTH, min_width: CONSTRAINT_GRID_WIDTH },
            ],
        }];
        let minimum = panel_width(&[BUTTON_MIN_WIDTH, CONSTRAINT_GRID_WIDTH]) + PANEL_GAP;
        let narrow = fit(&panels, minimum);
        assert_eq!(narrow[0].shown, 2);
        assert_eq!(narrow[0].widths, [BUTTON_MIN_WIDTH, CONSTRAINT_GRID_WIDTH]);
        let collapsed = fit(&panels, minimum - 1.0);
        assert_eq!((collapsed[0].fit, collapsed[0].shown), (Fit::Collapsed, 1));
    }

    fn captured_frame(h: &mut crate::scenario::Harness) {
        let token = h.app.shot_token;
        crate::issue_report::receive(&mut h.app, token, std::sync::Arc::new(egui::ColorImage::new([2, 2], vec![Color32::GREEN; 4])));
        h.frames(3);
    }

    #[test]
    fn panel_accents_share_the_tab_mapping_but_constraints_keep_their_role() {
        for dark in [false, true] {
            for vision in crate::theme::ColorVision::ALL {
                let t = Tokens::themed(dark, vision);
                for tab in TABS.iter().copied().chain(["SKETCH", "FORM", "RENDER"]) {
                    assert_eq!(panel_accent(&t, tab, "CREATE"), crate::theme::TabAccent::for_tab(&t, tab).color);
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
                for captions in [0.0, crate::titlebar::WIDTH] {
                    // The English label, and a longer one (Russian is about 125 px).
                    for label_width in [80.0, 125.0] {
                        let (report, labeled) = report_rect(bar, captions, 354.0, label_width, 40.0);
                        assert!(bar.contains_rect(report));
                        let prefs = Rect::from_center_size(pos2(report.left() - 52.0, APP_BAR_HEIGHT / 2.0), vec2(32.0, 32.0));
                        let help = Rect::from_center_size(pos2(help_x(report), APP_BAR_HEIGHT / 2.0), vec2(32.0, 32.0));
                        assert!(!report.intersects(prefs) && !report.intersects(help) && !help.intersects(prefs));
                        assert!(prefs.right() < help.left() && help.right() < report.left());
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
            let room = version_room(bar, crate::titlebar::WIDTH, 354.0, version).unwrap_or(8.0);
            2.0 * (1400.0 - crate::titlebar::WIDTH - room - center.x)
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
            for captions in [0.0, crate::titlebar::WIDTH] {
                // A release keeps the button where it always was.
                assert_eq!(version_room(bar, captions, 354.0, release), Some(RIGHT_GROUP), "{width} {captions}");
                for (k, version_width) in [release, candidate, 2.0 * candidate].into_iter().enumerate() {
                    let (report, _) = report_rect(bar, captions, 354.0, 80.0, version_width);
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
            // Existing review content scrolls vertically when the window is narrow.
            if width < 700.0 {
                let [x, y] = h.widget("Close draft").unwrap();
                h.call("ui.scroll", json!({"x":x,"y":y - 60.0,"delta":-1200}));
                // Let the scroll animation finish before locating the click target.
                h.frames(60);
            }
            h.step(&json!({"widget":"Close draft"})).unwrap();
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
        h.app.cam.target = solidcraft_engine::geom::Vec3::new(10.0, 0.0, 0.0);
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
        h.ctx.memory_mut(|m| m.request_focus(egui::Id::new("issue_report_body")));
        h.step(&json!({"text":"Describe this dimension problem."})).unwrap();
        h.step(&json!({"key":"Enter"})).unwrap();
        assert_eq!(before, (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.revision));
        h.step(&json!({"widget":"Close draft"})).unwrap();
        h.frames(2);
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(egui::Id::new("sc_dim_edit")));
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
                    h.ctx.data_mut(|d| d.insert_temp(egui::Id::new("sc_file_menu"), false));
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
        h.ctx.data_mut(|d| d.insert_temp(egui::Id::new("sc_file_menu"), false));
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
        h.ctx.data_mut(|d| d.insert_temp(egui::Id::new("sc_file_menu"), false));
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
        let english = crate::i18n::Locale::for_language("en").text("ui.needs_design");
        assert!(english.contains("{command}"));
        for catalog in crate::i18n::LOCALES.iter().filter(|c| !matches!(c.id, "en" | "en-GB")) {
            let text = crate::i18n::Locale::for_language(catalog.id).text("ui.needs_design");
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

    #[test]
    fn sketch_create_family_rows_paint_the_first_variants_icons() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        let cmds: Vec<_> = solidcraft_engine::command_specs().into_iter().filter(|c| c.tab == "SKETCH" && c.panel == "CREATE").collect();
        crate::icon_svg::audit::start();
        let mut output = h.ctx.run_ui(egui::RawInput::default(), |ui| {
            sketch_create_menu(&mut h.app, ui, &cmds, &mut None, &mut None);
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
        for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
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
            for vision in crate::theme::ColorVision::ALL {
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

    fn tab_test_themes() -> Vec<Tokens> {
        let mut themes = Vec::new();
        for dark in [false, true] {
            for vision in crate::theme::ColorVision::ALL {
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
                let accent = crate::theme::TabAccent::for_tab(&t, tab);
                for hovered in [false, true] {
                    let active = ribbon_tab_style(&t, tab, true, true, hovered);
                    let inactive = ribbon_tab_style(&t, tab, true, false, hovered);
                    assert_eq!(active, RibbonTabStyle { fill: accent.color, ink: accent.on_fill(&t), outline: Color32::TRANSPARENT });
                    assert_eq!(inactive, RibbonTabStyle { outline: accent.color, ..ribbon_tab_style(&t, tab, false, false, hovered) });
                    assert!(crate::theme::contrast_ratio(active.ink, active.fill) >= 4.5, "{tab}: selected text must remain readable");
                    // The selected contextual tab is the solid pill; the unselected one never is.
                    assert!(
                        crate::theme::contrast_ratio(active.fill, t.toolbar)
                            > crate::theme::contrast_ratio(if hovered { t.hover } else { t.toolbar }, t.toolbar),
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
                let accent = crate::theme::TabAccent::for_tab(&t, tab);
                for hovered in [false, true] {
                    let none = Color32::TRANSPARENT;
                    assert_eq!(ribbon_tab_style(&t, tab, false, true, hovered), RibbonTabStyle { fill: accent.soft, ink: accent.ink, outline: none });
                    let idle = if hovered { t.hover } else { none };
                    assert_eq!(ribbon_tab_style(&t, tab, false, false, hovered), RibbonTabStyle { fill: idle, ink: t.text, outline: none });
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
                ribbon_text(ui, pos2(100.3, 40.7), "Selected tab", 13.0, ink, true);
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
            assert_eq!(text.galley.job.sections[0].format.font_id, crate::fonts::semibold(13.0));
            assert_eq!(text.galley.job.sections[0].format.color, ink);
            assert!((text.pos.x * scale - (text.pos.x * scale).round()).abs() < 0.001);
            assert!((text.pos.y * scale - (text.pos.y * scale).round()).abs() < 0.001);
        }
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
                        let locale = crate::i18n::Locale::for_language(catalog.id);
                        let label = locale.label(crate::workspace::ribbon_button_label(command));
                        let width = ribbon_button_width(&painter, label) - BUTTON_LABEL_PADDING;
                        let galley = ribbon_button_galley(&painter, label, width, Color32::WHITE);
                        assert!(galley.rows.len() <= 2, "{}: {label}", command.id);
                        assert!(galley.size().x <= width + 0.01, "{}: {label}: {} > {width}", command.id, galley.size().x);
                        if crate::bidi::has_rtl(label) {
                            // epaint adds zero-width glyphs to right-to-left rows, so their text
                            // is not the source text: check the lines the galley is built from.
                            let lines = ribbon_button_lines(&painter, label, width);
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
                                    (ch.is_whitespace() || ch == '/' || cjk_word_break(label, end))
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
                let width = ribbon_button_width(&painter, label);
                assert_eq!(ribbon_button_lines(&painter, label, width - BUTTON_LABEL_PADDING), vec![label]);
                assert!((BUTTON_MIN_WIDTH..=BUTTON_MAX_WIDTH).contains(&width));
            }
            let finish = solidcraft_engine::find_command("sketch.finish").unwrap();
            let label = crate::workspace::ribbon_button_label(finish);
            let width = ribbon_button_width(&painter, label);
            assert_eq!(ribbon_button_lines(&painter, label, width - BUTTON_LABEL_PADDING), vec!["Finish", "Sketch"]);
            // Exercise full labels and pinned commands too, not just compact defaults.
            for (label, width, expected) in [
                ("Coincident", 50.0, vec!["Coincident"]),
                ("Rectangular Pattern", 50.0, vec!["Rectangular", "Pattern"]),
                ("Move/Copy", 40.0, vec!["Move/", "Copy"]),
            ] {
                let lines = ribbon_button_lines(&painter, label, width);
                // Exact glyph widths depend on the bundled font: verify the source
                // word stays on its own line, with only an optional suffix omitted.
                assert_eq!(lines.len(), expected.len(), "{label}: {lines:?}");
                for (actual, word) in lines.iter().zip(expected) {
                    assert!(actual == word || actual.strip_suffix('…').is_some_and(|prefix| word.starts_with(prefix)), "{label}: {lines:?}");
                }
            }
            let lines = ribbon_button_lines(&painter, "one two three four five", 50.0);
            assert_eq!(lines.len(), 2);
            assert!(lines[1].ends_with('…'));
            let lines = ribbon_button_lines(&painter, "長い日本語のラベル 続き", 50.0);
            assert_eq!(lines.len(), 2);
            assert!(lines[0].ends_with('…'));
            let ja = crate::i18n::Locale::for_language("ja");
            for (id, expected) in [
                ("sketch.create", vec!["新規", "スケッチ"]),
                ("config.table", vec!["構成", "テーブル"]),
                ("construct.plane.offset", vec!["オフセット", "平面"]),
                ("surface.loft", vec!["サーフェス", "ロフト"]),
                ("mesh.convert", vec!["メッシュを", "変換"]),
                ("component.create", vec!["新規", "コンポ"]),
                ("joint.origin", vec!["ジョイント", "原点"]),
                ("joint.drive", vec!["ジョイント", "を駆動"]),
            ] {
                let command = solidcraft_engine::find_command(id).unwrap();
                let label = ja.label(crate::workspace::ribbon_button_label(command));
                let width = ribbon_button_width(&painter, label) - BUTTON_LABEL_PADDING;
                assert_eq!(ribbon_button_lines(&painter, label, width), expected, "{id}");
            }
            let comp = solidcraft_engine::find_command("component.create").unwrap();
            assert_eq!(ja.label(comp.label), "新規コンポーネント");
            // Hebrew comes in display order (`bidi::visual`): the reader starts at the right end,
            // so "סקיצה חדשה" (New Sketch, stored as "חדשה סקיצה") puts סקיצה on the first line.
            let label = crate::bidi::visual("סקיצה חדשה").into_owned();
            let width = ribbon_label_width(&painter, "סקיצה").max(ribbon_label_width(&painter, "חדשה")) + 1.0;
            assert!(ribbon_label_width(&painter, &label) > width);
            assert_eq!(ribbon_button_lines(&painter, &label, width), vec!["סקיצה", "חדשה"]);
            let lines = ribbon_button_lines(&painter, &crate::bidi::visual("אחת שתיים שלוש ארבע חמש"), 50.0);
            assert_eq!(lines.len(), 2);
            assert!(lines[1].starts_with('…'), "{lines:?}");
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
        for sketch in [false, true] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.app.custom_titlebar = true;
            h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
            if sketch {
                h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            }
            let tab = if sketch { "SKETCH" } else { "MODEL" };
            h.app.ui.tab = tab.into();
            for w in [1920.0, 1600.0, 1440.0, 1366.0, 1280.0, 1100.0, 900.0, 700.0] {
                h.call("ui.resize", json!({"width":w,"height":900}));
                h.frames(3);
                let at = format!("{tab} at {w}");
                let bar = crate::scenario::rect_of("toolbar").unwrap();
                let finish = crate::scenario::rect_of("panel:SKETCH:FINISH SKETCH");
                // The room for the other panels: the bar less the Finish group kept for it.
                let right = bar.right() - 4.0 - finish.map_or(0.0, |f| f.width() + 4.0);
                let count = |name: &str| crate::scenario::count_of(name).unwrap_or(-1.0);
                let (collapsed, trimmed, more) = (count("toolbar_collapsed"), count("toolbar_trimmed"), count("toolbar_more"));
                let lists = crate::toolbar_custom::panel_lists(tab);
                let mut end = bar.left();
                let mut on_bar = 0;
                for (panel, cmds, promote) in &lists {
                    let Some(rect) = crate::scenario::rect_of(&format!("panel:{tab}:{panel}")) else { continue };
                    on_bar += 1;
                    if *panel != "FINISH SKETCH" {
                        end = end.max(rect.right() + PANEL_GAP);
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
                let gap = right - end - if more > 0.0 { MORE_ROOM } else { 0.0 };
                assert!(gap >= -0.5, "{at}: the panels run {} px past the room", -gap);
                let hidden = collapsed + trimmed + more > 0.0;
                let button = if more > 0.0 {
                    COLLAPSED_WIDTH + PANEL_GAP
                } else if sketch {
                    CONSTRAINT_GRID_WIDTH + BUTTON_GAP
                } else {
                    BUTTON_MIN_WIDTH + BUTTON_GAP
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
                // The assistant lives in the title bar; visible ribbon tabs fit before search.
                let search = crate::scenario::rect_of("ribbon:search").unwrap();
                for t in TABS.iter().chain(["SKETCH"].iter()) {
                    if let Some(r) = crate::scenario::rect_of(&format!("toolbar_tab:{t}")) {
                        assert!(r.right() < search.left(), "{at}: the {t} tab runs into search");
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
        assert_eq!(panel.width(), COLLAPSED_WIDTH, "the panel takes a collapsed panel's room");
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
