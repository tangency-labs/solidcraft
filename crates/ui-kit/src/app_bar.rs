//! The application bar as components: the bar ([`show`]) and its document tabs ([`tabs`]). The bar: the app's mark and name, Home, the File button, the file
//! actions (Save, Undo, Redo…), room for the app's quick-access buttons, and at the right Help,
//! Settings, Report a bug, the command search's place and the version. The app describes the bar
//! as plain data ([`AppBar`]); [`show`] draws it inside the title bar's content rect and returns
//! what was chosen ([`BarAction`]) with the places it leaves to the app ([`BarOut`]): its quick
//! access buttons, an offer pill, the document tabs, the assistant and the search field. Its parts
//! are published in the widget registry under the names the app gives ([`Handles`]).

use crate::BackingTooltipExt as _;
use egui::{Align2, FontId, Painter, Pos2, Rect, Sense, Stroke, pos2, vec2};
use solidcraft_theme::Tokens;

/// The bar's height: the window's title bar on desktop builds.
pub const HEIGHT: f32 = 34.0;
/// Room for the version between Report a bug and the caption buttons, at the least.
pub const RIGHT_GROUP: f32 = 60.0;
/// The right-hand cluster's buttons (Assistant, Settings, Help) before Report a bug.
pub const CLUSTER_BUTTONS: f32 = 108.0;
/// The least room the document tabs keep when a long version takes its extra width: one tab and
/// its + button.
const TABS_KEPT: f32 = 124.0;
/// Where the Report a bug text starts, from the button's left edge.
pub const REPORT_TEXT_X: f32 = 29.0;
/// The fills of the bar's pills. They are the services' own colours (Ko-fi coral, Discord
/// blurple), the same in every theme; both take white ink.
pub mod brand {
    use egui::Color32;
    pub const KOFI: Color32 = Color32::from_rgb(0xFF, 0x5E, 0x5B);
    pub const DISCORD: Color32 = Color32::from_rgb(0x58, 0x65, 0xF2);
}
/// The pills' height, and the least width at which the bar gives the first pill its label.
pub const PILL_HEIGHT: f32 = 24.0;
pub const PILL_LABEL_MIN: f32 = 1100.0;
/// The gap before and between pills, and the × inside one.
const PILL_GAP: f32 = 6.0;
const PILL_X: f32 = 16.0;

/// Space between the search field and Settings.
pub const SEARCH_GAP: f32 = 4.0;
/// Home's least place from the bar's left edge, and the least gap between the app's name and
/// Home's button. 128 was Home's fixed place, sized for "SolidCraft": the name starts at 36 and is
/// 65.1 wide in Plex Sans semibold at the theme's heading size (14), so it ends at 101.1, and Home's
/// button starts at 108 (20 before its place), 6.9 after it. A longer name moves Home right by
/// the same gap, rounded down so SolidCraft's bar stays put: 36 + 65.1 + 6 + 20 = 127.1, under 128.
const HOME_MIN: f32 = 128.0;
const NAME_GAP: f32 = 6.0;
/// A bar narrower than this shows no product name, no Undo and Redo, and narrower file actions.
const COMPACT: f32 = 640.0;
/// Space the File menu keeps below its rows for its frame and a gap above the window's bottom.
const FILE_MENU_BOTTOM_GAP: f32 = 24.0;
/// The least height the File menu's rows get, about three rows, however short the window.
const FILE_MENU_MIN_ROOM: f32 = 100.0;

/// Where the bar publishes its parts in the widget registry.
#[derive(Clone, Copy, Debug)]
pub struct Handles<'a> {
    /// Home's centre under each of these names, and its rect under each of `home_rect`.
    pub home: &'a [&'a str],
    pub home_rect: &'a [&'a str],
    /// The File button, likewise.
    pub file: &'a [&'a str],
    pub file_rect: &'a [&'a str],
    /// The Plugins button's rect and centre, when the bar has one.
    pub plugins: &'a str,
    /// A button's rect as a target, under each prefix followed by its id.
    pub button: &'a [&'a str],
    /// A separator's rect, this prefix followed by its group.
    pub separator: &'a str,
    pub search: &'a str,
    pub report: &'a str,
    /// A pill's rect under this prefix followed by its id, and its × under that followed by `:hide`.
    pub pill: &'a str,
    /// Settings' rect and centre.
    pub settings: &'a str,
    pub version: &'a str,
}

/// A bar button: an icon with its hover text.
#[derive(Clone, Copy, Debug)]
pub struct BarButton<'a> {
    /// Its name in handles and the action it returns.
    pub id: &'a str,
    /// A kit icon name.
    pub icon: &'a str,
    pub tip: &'a str,
    pub enabled: bool,
    /// Narrower on a compact bar.
    pub narrow: bool,
    /// Left out of a compact bar (its shortcut and menus remain).
    pub wide_only: bool,
}

/// Report a bug: its label, its hover text when labelled, and whether it is always the icon.
#[derive(Clone, Copy, Debug)]
pub struct Report<'a> {
    pub label: &'a str,
    pub tip: &'a str,
    pub icon_only: bool,
}

/// A link pill before Report a bug: a flat coloured capsule with a mark, a label while the bar is
/// wide and a small × on its right that asks to hide it.
#[derive(Clone, Copy, Debug)]
pub struct Pill<'a> {
    /// Its name in handles and in the actions it returns.
    pub id: &'a str,
    pub icon: &'a str,
    /// Shown beside the mark on a wide bar (`labelled`); empty for an icon-only pill.
    pub label: &'a str,
    pub labelled: bool,
    pub tip: &'a str,
    pub hide_tip: &'a str,
    /// The accessible names of the pill and of its ×.
    pub name: &'a str,
    pub hide_name: &'a str,
    pub fill: egui::Color32,
    pub ink: egui::Color32,
}

/// The version at the bar's right end, linking to `url`.
#[derive(Clone, Copy, Debug)]
pub struct Version<'a> {
    pub text: &'a str,
    pub url: &'a str,
}

/// The bar as the app describes it this frame.
pub struct AppBar<'a> {
    /// The app's mark, painted centred at the point given, and its name beside it.
    pub mark: &'a dyn Fn(&Painter, Pos2),
    pub name: &'a str,
    pub home: BarButton<'a>,
    /// The File button's label; it is enabled with `home`, and drawn held while its menu is open.
    pub file: &'a str,
    pub file_open: bool,
    /// The Plugins menu's text button after File, and whether its menu is open; `None` leaves it
    /// out (the web build, which runs no plugins). Its accessible name is the third member
    /// ("Plugins menu"), which contains the visible label but is not the Preferences page's name.
    pub plugins: Option<(&'a str, bool, &'a str)>,
    /// The file actions after File.
    pub actions: Vec<BarButton<'a>>,
    /// How many quick-access buttons the app draws after them (one `step` each).
    pub quick: usize,
    pub help: BarButton<'a>,
    pub settings: BarButton<'a>,
    pub report: Report<'a>,
    /// The link pills, left to right, just before Report a bug.
    pub pills: Vec<Pill<'a>>,
    /// The bar holds the command search field, just before Settings.
    pub search: bool,
    /// The shortcut the closed search field shows, as the keymap displays it; it sets the field's
    /// width.
    pub search_shortcut: Option<&'a str>,
    /// The bar keeps a place for the app's assistant button.
    pub assistant: bool,
    pub version: Option<Version<'a>>,
    pub handles: Handles<'a>,
}

/// A menu's text button (File, Plugins), its label starting 5 before `x`: its rect, and whether it
/// was clicked while `idle`. Drawn held while its menu is `open`.
/// It shows `label`; `name` is its accessible name.
fn text_button(ui: &mut egui::Ui, r: Rect, x: f32, (label, name): (&str, &str), open: bool, idle: bool, salt: &str) -> (Rect, bool) {
    let t = Tokens::get();
    let galley = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(t.font_button), t.app_bar_text);
    let rect = Rect::from_min_size(pos2(x - 13.0, r.center().y - 13.0), vec2(galley.size().x + 16.0, 26.0));
    let resp = ui.interact(rect, ui.scope_id().with(("ab", salt)), if idle { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, idle, name));
    if idle && (resp.hovered() || resp.has_focus() || open) {
        ui.painter().rect_filled(rect, t.radius_sm, t.app_bar_hover);
    }
    let ink = if idle { t.app_bar_text } else { t.app_bar_text.gamma_multiply(0.35) };
    ui.painter().galley(pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0), galley, ink);
    (rect, idle && resp.clicked())
}

/// What the user chose on the bar this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum BarAction {
    Home,
    /// The File button, at `rect`.
    File {
        rect: Rect,
    },
    /// The Plugins button, at `rect`.
    Plugins {
        rect: Rect,
    },
    /// A file action or Settings, by its id.
    Button {
        id: String,
    },
    /// Help, its menu anchored at `x`.
    Help {
        x: f32,
    },
    Report,
    /// A pill was clicked / its × was.
    Pill {
        id: String,
    },
    HidePill {
        id: String,
    },
    /// The version's link.
    Version,
}

/// The bar drawn: what was chosen, and the places it leaves to the app.
#[derive(Debug)]
pub struct BarOut {
    pub actions: Vec<BarAction>,
    /// The File button, where its menu opens.
    pub file: Rect,
    /// The quick-access buttons: the first one's centre x and the step between them.
    pub quick: Option<(f32, f32)>,
    /// Where an offer pill may go (right-aligned in it), and whether it may carry its label.
    pub offer: Rect,
    pub offer_labeled: bool,
    /// The document tabs run from here to the offer pill (or `controls_left` without one).
    pub tabs_start: f32,
    pub controls_left: f32,
    pub search: Option<Rect>,
    pub assistant: Option<Rect>,
    /// The bar had no room for the Plugins button: the app lists Plugins in the File menu.
    pub plugins_in_file: bool,
}

fn handle(prefix: &str, key: &str) -> String {
    format!("{prefix}{key}")
}

/// The room right of Report a bug for the version (`version_width`, measured), or None where the
/// bar shows none. The version sits 12 points from the caption buttons with a gap before the
/// button, so a longer one moves the button left instead of covering it. Its width over
/// [`RIGHT_GROUP`] comes only from the document tabs' spare room: where it would leave them less
/// than [`TABS_KEPT`], the version is hidden, as it is on a narrow bar.
pub fn version_room(bar: Rect, captions: f32, tabs_start: f32, version_width: f32) -> Option<f32> {
    if bar.width() < COMPACT {
        return None;
    }
    let need = 12.0 + version_width + 8.0;
    let room = RIGHT_GROUP.max(need);
    // The tabs end 8 points before the cluster, with Report a bug as an icon at the least.
    let tabs = bar.right() - captions - room - 28.0 - CLUSTER_BUTTONS - 8.0 - tabs_start;
    (need <= RIGHT_GROUP || tabs >= TABS_KEPT).then_some(room)
}

/// The centre of Help, between Settings and Report a bug.
pub fn help_x(report: Rect) -> f32 {
    report.left() - 18.0
}

/// The Report a bug button: labelled unless `icon_only`, when there is room for its text
/// (`label_width`, measured), otherwise just the icon. An icon-only button is a square the size of
/// Settings and Help. `version_width`: the measured version, when the bar shows it.
pub fn report_rect(bar: Rect, captions: f32, tabs_start: f32, label_width: f32, version_width: Option<f32>, icon_only: bool) -> (Rect, bool) {
    let room = version_width.and_then(|width| version_room(bar, captions, tabs_start, width));
    let right = bar.right() - captions - room.unwrap_or(8.0);
    if icon_only {
        return (Rect::from_min_size(pos2(right - 32.0, bar.center().y - 16.0), vec2(32.0, 32.0)), false);
    }
    // Icon, text and a margin that mirrors the icon side.
    let full = REPORT_TEXT_X + label_width + 11.0;
    let labeled = right - CLUSTER_BUTTONS - tabs_start >= 216.0 + full;
    let width = if labeled { full } else { 28.0 };
    (Rect::from_min_size(pos2(right - width, bar.center().y - 13.0), vec2(width, 26.0)), labeled)
}

/// The closed search field in the bar, ending `right`: whole when the document tabs keep the room
/// Report a bug's label leaves them (`tabs_room`: theirs without the field), else just its
/// magnifier.
pub fn search_rect(ctx: &egui::Context, shortcut: Option<&str>, bar: Rect, right: f32, tabs_room: f32) -> Rect {
    let height = 26.0;
    let full = crate::CommandSearch::closed_width(ctx, shortcut);
    let whole = bar.width() >= COMPACT && tabs_room - full - SEARCH_GAP >= 216.0;
    let width = if whole { full } else { crate::CommandSearch::compact_width(height) };
    Rect::from_min_size(pos2(right - width, bar.center().y - height / 2.0), vec2(width, height))
}

/// A pill's width: pad, mark, label, ×.
fn pill_width(ui: &egui::Ui, p: &Pill) -> f32 {
    let label = if p.labelled && !p.label.is_empty() {
        5.0 + ui.painter().layout_no_wrap(p.label.to_string(), crate::type_face::semibold(Tokens::get().font_label), p.ink).size().x
    } else {
        0.0
    };
    8.0 + 14.0 + label + 4.0 + PILL_X + 3.0
}

/// The room the pills take left of Report a bug: each with the gap before it.
fn pills_width(ui: &egui::Ui, pills: &[Pill]) -> f32 {
    pills.iter().map(|p| pill_width(ui, p) + PILL_GAP).sum()
}

/// A flat capsule `PILL_HEIGHT` tall in `fill`, a touch lighter while `hot`.
fn capsule(ui: &egui::Ui, rect: Rect, fill: egui::Color32, hot: bool) {
    let fill = if hot { fill.lerp_to_gamma(egui::Color32::WHITE, 0.12) } else { fill };
    ui.painter().rect_filled(rect, egui::CornerRadius::same((rect.height() / 2.0) as u8), fill);
}

/// One pill, its right edge at `right`: returns its left edge. A flat capsule with full-height
/// corners; hovering lightens its fill by about 8 % over 0.1 s. The × is a plain glyph, with a
/// round translucent dark background (black at 38 %) that fades in while it is hovered. Edges are
/// snapped to whole pixels so they stay crisp at any scale.
fn pill(ui: &mut egui::Ui, bar: Rect, right: f32, p: &Pill, h: &Handles, actions: &mut Vec<BarAction>) -> f32 {
    let t = Tokens::get();
    let ppp = ui.ctx().pixels_per_point();
    let snap = |v: f32| (v * ppp).round() / ppp;
    let width = snap(pill_width(ui, p));
    let rect = Rect::from_min_size(pos2(snap(right - width), snap(bar.center().y - PILL_HEIGHT / 2.0)), vec2(width, PILL_HEIGHT));
    let x_rect = Rect::from_center_size(pos2(snap(rect.right() - 4.0 - PILL_X / 2.0), rect.center().y), vec2(PILL_X, PILL_X));
    let main = Rect::from_min_max(rect.min, pos2(x_rect.left(), rect.bottom()));
    let main_resp = ui.interact(main, ui.scope_id().with(("sc_pill", p.id)), Sense::click());
    let x_resp = ui.interact(x_rect, ui.scope_id().with(("sc_pill_hide", p.id)), Sense::click());
    main_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, p.name));
    x_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, p.hide_name));
    let (hot, hot_x) = (main_resp.hovered() || main_resp.has_focus() || x_resp.hovered(), x_resp.hovered() || x_resp.has_focus());
    let lift = ui.ctx().animate_bool_with_time(ui.scope_id().with(("sc_pill_lift", p.id)), hot, 0.1);
    let veil = ui.ctx().animate_bool_with_time(ui.scope_id().with(("sc_pill_veil", p.id)), hot_x, 0.1);
    let fill = p.fill.lerp_to_gamma(egui::Color32::WHITE, 0.08 * lift);
    ui.painter().rect_filled(rect, egui::CornerRadius::same((PILL_HEIGHT / 2.0) as u8), fill);
    if main_resp.has_focus() || x_resp.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(1.5),
            egui::CornerRadius::same((PILL_HEIGHT / 2.0 + 1.5) as u8),
            Stroke::new(1.5, t.app_bar_text),
            egui::StrokeKind::Outside,
        );
    }
    let icon = Rect::from_center_size(pos2(rect.left() + 8.0 + 7.0, rect.center().y), vec2(14.0, 14.0));
    crate::icons::paint_ink(ui.painter(), icon, p.icon, p.ink);
    if p.labelled && !p.label.is_empty() {
        // Centred on the capitals' height, not the line box, so the name sits level with the mark.
        let galley = ui.painter().layout_no_wrap(p.label.to_string(), crate::type_face::semibold(Tokens::get().font_label), p.ink);
        let top = rect.center().y - galley.size().y / 2.0 + 0.5;
        ui.painter().galley(pos2(snap(icon.right() + 5.0), snap(top)), galley, p.ink);
    }
    if veil > 0.0 {
        ui.painter().circle_filled(x_rect.center(), PILL_X / 2.0, egui::Color32::from_black_alpha((97.0 * veil) as u8));
    }
    crate::icons::paint_ink(ui.painter(), Rect::from_center_size(x_rect.center(), vec2(8.0, 8.0)), "close", p.ink);
    solidcraft_ui_registry::publish_target(&handle(h.pill, p.id), main);
    solidcraft_ui_registry::publish_target(&format!("{}{}:hide", h.pill, p.id), x_rect);
    if main_resp.on_hover_cursor(egui::CursorIcon::PointingHand).backing_on_hover_text(p.tip).clicked() {
        actions.push(BarAction::Pill { id: p.id.to_string() });
    }
    if x_resp.on_hover_cursor(egui::CursorIcon::PointingHand).backing_on_hover_text(p.hide_tip).clicked() {
        actions.push(BarAction::HidePill { id: p.id.to_string() });
    }
    rect.left() - PILL_GAP
}

/// A short divider between the bar's groups.
pub fn separator(ui: &egui::Ui, bar: Rect, x: f32, group: &str, handles: &Handles) {
    let t = Tokens::get();
    let rect = Rect::from_center_size(pos2(x, bar.center().y), vec2(1.0, 20.0));
    ui.painter().line_segment([rect.center_top(), rect.center_bottom()], Stroke::new(1.0, t.app_bar.lerp_to_gamma(t.app_bar_separator, 0.4)));
    solidcraft_ui_registry::publish_rect(&handle(handles.separator, group), rect);
}

/// An icon button of the bar, dimmed and deaf to clicks when disabled (its tip still shows).
/// Returns whether it was clicked.
fn icon_button(ui: &mut egui::Ui, bar: Rect, at: f32, b: &BarButton, compact: bool, handles: &Handles) -> bool {
    let t = Tokens::get();
    let width = if compact && b.narrow { 22.0 } else { 32.0 };
    let br = Rect::from_center_size(pos2(at, bar.center().y), vec2(width, 32.0));
    let resp = ui.interact(br, ui.scope_id().with(("ab", b.id)), if b.enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, b.enabled, b.tip));
    for prefix in handles.button {
        solidcraft_ui_registry::publish_target(&handle(prefix, b.id), br);
    }
    if b.enabled && (resp.hovered() || resp.has_focus()) {
        ui.painter().rect_filled(br.shrink(3.0), t.radius_sm, t.app_bar_hover_button);
    }
    crate::icons::paint_app_bar(ui.painter(), Rect::from_center_size(br.center(), vec2(18.0, 18.0)), b.icon, if b.enabled { 1.0 } else { 0.35 });
    let clicked = resp.backing_on_hover_text(b.tip).clicked();
    b.enabled && clicked
}

/// Draws the bar in `bar`, its controls within `content` (the title bar's room between the
/// window's own buttons).
pub fn show(ui: &mut egui::Ui, data: &AppBar, bar: Rect, content: Rect) -> BarOut {
    let t = Tokens::get();
    let h = &data.handles;
    let r = bar;
    let left = content.left();
    let compact = r.right() - left < COMPACT;
    // The Plugins menu's button shows its label where the bar has room for it. Between the compact
    // width and that room, a button would cost the bar a control it had before there was a Plugins
    // menu: the bar leaves it out and the app puts Plugins in the File menu instead. A compact
    // bar, which already drops controls, shows it as a narrow icon.
    let plugins_label = data.plugins.is_some_and(|(label, _, _)| {
        let width = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(t.font_button), t.app_bar_text).size().x + 18.0;
        r.right() - left >= COMPACT + width
    });
    let plugins_in_file = data.plugins.is_some() && !plugins_label && !compact;
    let mut actions = Vec::new();
    (data.mark)(ui.painter(), pos2(left + 20.0, r.center().y));
    // Home's place: after the name as drawn, and never before HOME_MIN.
    let mut x = if compact {
        left + 56.0
    } else {
        let name = ui.painter().text(
            pos2(left + 36.0, r.center().y),
            Align2::LEFT_CENTER,
            data.name,
            crate::type_face::semibold(t.font_heading),
            t.app_bar_text,
        );
        // Home's button starts 20 before its place (32 wide, centred 4 before it).
        (left + HOME_MIN).max(name.right() + NAME_GAP + 20.0)
    };
    // Home.
    let home = Rect::from_center_size(pos2(x - 4.0, r.center().y), vec2(32.0, 32.0));
    let idle = data.home.enabled;
    let home_response = ui.interact(home, ui.scope_id().with("appbar_home"), if idle { Sense::click() } else { Sense::hover() });
    home_response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, idle, data.home.tip));
    for name in h.home {
        solidcraft_ui_registry::publish_handle(name, home.center());
    }
    for name in h.home_rect {
        solidcraft_ui_registry::publish_rect(name, home);
    }
    if idle && (home_response.hovered() || home_response.has_focus()) {
        ui.painter().rect_filled(home.shrink(3.0), t.radius_sm, t.app_bar_hover_button);
    }
    crate::icons::paint_app_bar(ui.painter(), home.shrink(7.0), data.home.icon, if idle { 1.0 } else { 0.35 });
    if home_response.backing_on_hover_text(data.home.tip).clicked() && idle {
        actions.push(BarAction::Home);
    }
    x = home.right() + 8.0;
    separator(ui, r, x, "brand", h);
    x += 16.0;
    // The File menu's text button, then the Plugins menu's.
    let (file, clicked) = text_button(ui, r, x, (data.file, data.file), data.file_open, idle, "file");
    x = file.right() + 17.0;
    for name in h.file {
        solidcraft_ui_registry::publish_handle(name, file.center());
    }
    for name in h.file_rect {
        solidcraft_ui_registry::publish_rect(name, file);
    }
    if clicked {
        actions.push(BarAction::File { rect: file });
    }
    if let Some((label, open, name)) = data.plugins.filter(|_| !plugins_in_file) {
        let (plugins, clicked) = if plugins_label {
            let (plugins, clicked) = text_button(ui, r, x - 2.0, (label, name), open, idle, "plugins");
            x = plugins.right() + 17.0;
            (plugins, clicked)
        } else {
            let button = BarButton { id: "plugins", icon: "prefs_plugins", tip: name, enabled: idle, narrow: true, wide_only: false };
            let plugins = Rect::from_center_size(pos2(x, r.center().y), vec2(22.0, 32.0));
            if open {
                ui.painter().rect_filled(plugins.shrink(3.0), t.radius_sm, t.app_bar_hover_button);
            }
            let clicked = icon_button(ui, r, x, &button, compact, h);
            x += 24.0;
            (plugins, clicked)
        };
        solidcraft_ui_registry::publish_rect(h.plugins, plugins);
        solidcraft_ui_registry::publish_handle(h.plugins, plugins.center());
        if clicked {
            actions.push(BarAction::Plugins { rect: plugins });
        }
    }
    // The file actions, then the app's quick-access buttons.
    let step = if compact { 22.0 } else { 34.0 };
    for b in data.actions.iter().filter(|b| !(compact && b.wide_only)) {
        if icon_button(ui, r, x, b, compact, h) {
            actions.push(BarAction::Button { id: b.id.to_string() });
        }
        x += step;
    }
    let mut quick = None;
    if !compact && data.quick > 0 {
        separator(ui, r, x - step / 2.0 + 8.0, "quick_access", h);
        x += 8.0;
        quick = Some((x, step));
        x += data.quick as f32 * step;
    }
    // The document tabs follow the file actions.
    let captions = r.right() - content.right();
    let separator_x = x - step + if compact { 19.0 } else { 24.0 };
    separator(ui, r, separator_x, "documents", h);
    let tabs_start = separator_x + 10.0;
    let report_font = FontId::proportional(t.font_button);
    let label_width = ui.painter().layout_no_wrap(data.report.label.to_string(), report_font.clone(), t.app_bar_text).size().x;
    // The version links to its release notes; measured here so Report a bug leaves it room.
    let version = data.version.map(|v| (v, ui.painter().layout_no_wrap(v.text.to_string(), FontId::proportional(t.font_small), t.app_bar_text)));
    let version_width = version.as_ref().map(|(_, galley)| galley.size().x);
    let (report, report_label) = report_rect(r, captions, tabs_start, label_width, version_width, data.report.icon_only);
    // The right-hand controls keep their places; the link pills sit left of them, in the room the
    // document tabs leave (see `pills_w` below).
    let anchor = report.left();
    // Settings' left edge; the search field, when the bar holds it, sits just before it and moves
    // the assistant and the end of the document tabs left by its width.
    let gear_left = anchor - 68.0;
    let search = data.search.then(|| search_rect(ui.ctx(), data.search_shortcut, r, gear_left - SEARCH_GAP, anchor - CLUSTER_BUTTONS - tabs_start));
    let search_shift = search.map_or(0.0, |s| s.width() + SEARCH_GAP);
    if let Some(search) = search {
        solidcraft_ui_registry::publish_target(h.search, search);
    }
    if icon_button(ui, r, anchor - 18.0, &data.help, compact, h) {
        actions.push(BarAction::Help { x: anchor - 18.0 });
    }
    // An offer (a newer release) sits before the right cluster, in the space up to the file
    // actions; labelled where Report a bug is (an icon-only one leaves it to fit by room).
    let mut controls_left = anchor - CLUSTER_BUTTONS - search_shift;
    // The link pills end 6 before the assistant. They are left out where the tabs would keep less
    // than one tab's room (216) beside them, so a narrow bar looks as it did without them.
    let pills_w = pills_width(ui, &data.pills);
    let pills_shown = !data.pills.is_empty() && controls_left - pills_w - tabs_start >= 216.0;
    let pills_right = controls_left - 4.0;
    if pills_shown {
        controls_left -= pills_w;
    }
    let offer = Rect::from_min_max(pos2(x, report.top()), pos2(controls_left - 2.0, report.bottom()));
    let offer_labeled = report_label || (data.report.icon_only && r.width() >= COMPACT);
    // Report a bug.
    let response = ui.interact(report, ui.scope_id().with("sc_report_button"), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), data.report.label));
    solidcraft_ui_registry::publish_target(h.report, report);
    let hot = response.hovered() || response.has_focus();
    // A neutral pill shaped like the link pills: the bug mark, and the label where there is room.
    let capsule_rect = Rect::from_center_size(report.center(), vec2(report.width().min(if report_label { f32::MAX } else { 28.0 }), PILL_HEIGHT));
    // Neutral: a raised control in the theme's own colours, the same shape as the link pills.
    let (neutral, ink) = (t.app_bar.lerp_to_gamma(t.app_bar_text, 0.12), t.app_bar_text);
    capsule(ui, capsule_rect, neutral, hot);
    let tip = if report_label {
        let icon = Rect::from_center_size(pos2(capsule_rect.left() + 14.0, capsule_rect.center().y), vec2(16.0, 16.0));
        crate::icons::paint_ink(ui.painter(), icon, "bug_report", ink);
        ui.painter().text(
            pos2(capsule_rect.left() + REPORT_TEXT_X, capsule_rect.center().y),
            Align2::LEFT_CENTER,
            data.report.label,
            report_font,
            ink,
        );
        data.report.tip
    } else {
        crate::icons::paint_ink(ui.painter(), Rect::from_center_size(capsule_rect.center(), vec2(16.0, 16.0)), "bug_report", ink);
        if data.report.icon_only { data.report.label } else { data.report.tip }
    };
    if response.backing_on_hover_text(tip).clicked() {
        actions.push(BarAction::Report);
    }
    // The link pills, left of the assistant.
    let mut right = pills_right;
    for p in data.pills.iter().rev().filter(|_| pills_shown) {
        right = pill(ui, r, right, p, h, &mut actions);
    }
    // Settings sits immediately left of Help.
    let gear = Rect::from_center_size(pos2(anchor - 52.0, r.center().y), vec2(32.0, 32.0));
    let gresp = ui.interact(gear, ui.scope_id().with("sc_prefs_button"), Sense::click());
    if gresp.hovered() || gresp.has_focus() {
        ui.painter().rect_filled(gear.shrink(3.0), t.radius_sm, t.app_bar_hover_button);
    }
    crate::icons::paint_app_bar(ui.painter(), gear.shrink(7.0), data.settings.icon, 1.0);
    solidcraft_ui_registry::publish_rect(h.settings, gear);
    let assistant = data.assistant.then(|| Rect::from_center_size(pos2(anchor - 86.0 - search_shift, r.center().y), vec2(32.0, 32.0)));
    solidcraft_ui_registry::publish_handle(h.settings, gear.center());
    if gresp.backing_on_hover_text(data.settings.tip).clicked() {
        actions.push(BarAction::Button { id: data.settings.id.to_string() });
    }
    if let Some((v, galley)) = version
        && version_room(r, captions, tabs_start, galley.size().x).is_some()
    {
        let vrect = Align2::RIGHT_CENTER.anchor_size(pos2(r.right() - 12.0 - captions, r.center().y), galley.size());
        let vresp = ui.interact(
            vrect.expand2(vec2(2.0, 4.0)).intersect(Rect::everything_right_of(report.right())),
            ui.scope_id().with("sc_version_link"),
            Sense::click(),
        );
        let vresp = vresp.on_hover_cursor(egui::CursorIcon::PointingHand).backing_on_hover_text(v.url);
        if vresp.hovered() || vresp.has_focus() {
            ui.painter().hline(vrect.x_range(), vrect.bottom(), Stroke::new(1.0, t.app_bar_text));
        }
        ui.painter().galley(vrect.min, galley, t.app_bar_text);
        solidcraft_ui_registry::publish_handle(h.version, vrect.center());
        if vresp.clicked() {
            actions.push(BarAction::Version);
        }
    }
    BarOut { actions, file, quick, offer, offer_labeled, tabs_start, controls_left, search, assistant, plugins_in_file }
}

/// Where the tab strip publishes its parts: each prefix is followed by the tab's place.
#[derive(Clone, Copy, Debug)]
pub struct TabHandles<'a> {
    pub tab: &'a str,
    pub close: &'a str,
    /// The » button listing every tab, and the + button.
    pub more: &'a str,
    pub new: &'a str,
    /// The name field while a tab is renamed.
    pub rename: &'a str,
    /// A row of a tab's menu, this prefix followed by the row's id; a row of the » list, by place.
    pub menu_row: &'a str,
    pub more_row: &'a str,
}

/// One document tab.
#[derive(Clone, Copy, Debug)]
pub struct DocTab<'a> {
    /// The name the tab shows, and the one the » list shows.
    pub name: &'a str,
    pub list_name: &'a str,
    /// It has unsaved changes (a bullet after its name).
    pub dirty: bool,
    /// Its hover text (where it is saved); its full name goes above it when the tab cuts it short.
    pub place: &'a str,
    /// What the file's signing provenance says, as a shield beside the name: persistent, where
    /// the tooltip can only be read by hovering. `None` for an unsigned design (or none checked).
    pub security: Option<TabBadge>,
}

/// The state a design tab's shield shows. The colour carries the state; the tooltip has the words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabBadge {
    /// Records are there but have not been checked in this session.
    Signed,
    /// Checked: every record verifies and covers the file.
    Verified,
    /// Signed earlier, but the file was saved again without a key.
    NotCovered,
    /// A record does not verify.
    Invalid,
    /// A newer SolidCraft's record format.
    Unsupported,
}

impl TabBadge {
    fn ink(self, t: &solidcraft_theme::Tokens) -> egui::Color32 {
        match self {
            Self::Verified => t.agent_ok,
            Self::Signed => t.app_bar_text.gamma_multiply(0.75),
            Self::NotCovered | Self::Unsupported => t.warning,
            Self::Invalid => t.error,
        }
    }
}

/// A row of a tab's menu (Close, Close Other Tabs…), with its label and the key shown when the
/// menu is the selected tab's.
#[derive(Clone, Copy, Debug)]
pub struct TabMenuRow<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub key_on_selected: &'a str,
    /// Offered only while there are other tabs.
    pub needs_others: bool,
}

/// The selected tab's name being edited: its text and whether the field has taken the focus.
pub struct TabRename<'a> {
    pub text: &'a mut String,
    pub focused: &'a mut bool,
    /// The longest name taken, in characters.
    pub max: usize,
}

/// The tab strip as the app describes it this frame.
pub struct TabStrip<'a> {
    pub tabs: Vec<DocTab<'a>>,
    /// The selected tab (the strip keeps it in view), and the one drawn selected (None while the
    /// app shows something else, its start page).
    pub selected: usize,
    pub shown_selected: Option<usize>,
    /// The tabs are part of the window's title bar: dragging the only tab, or one mostly up or
    /// down, moves the window, and a double click maximizes it (unless it renames).
    pub title_bar: bool,
    /// A double click on the selected tab renames it.
    pub double_click_renames: bool,
    pub rename: Option<TabRename<'a>>,
    pub menu: &'a [TabMenuRow<'a>],
    /// Hover texts and names: the close button's accessible name and tip, the » button's and the
    /// + button's tips.
    pub close_name: &'a str,
    pub close_tip: &'a str,
    pub more_tip: &'a str,
    pub new_tip: &'a str,
    /// Where the strip keeps its drag, menu and list state.
    pub id: egui::Id,
    pub handles: TabHandles<'a>,
}

/// Whether the strip that keeps its state at `id` ([`TabStrip::id`]) has its list of every tab
/// open.
pub fn tab_list_open(ctx: &egui::Context, id: egui::Id) -> bool {
    ctx.data(|d| d.get_temp::<bool>(id.with("more")).unwrap_or(false))
}

/// What the user chose on the tab strip this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum TabAction {
    Select(usize),
    Close(usize),
    /// A row of tab `tab`'s menu.
    Menu {
        tab: usize,
        id: String,
    },
    /// A tab dragged from one place to another.
    Reorder {
        from: usize,
        to: usize,
    },
    /// Rename tab `tab` in place (its rect, where a menu would open).
    StartRename {
        tab: usize,
        rect: Rect,
    },
    /// The name typed was kept, or the edit given up.
    RenameDone,
    RenameCancelled,
    Maximize,
    New,
}

/// A tab dragged along the strip: the tab and where along it the pointer grabbed it.
#[derive(Clone, Copy, Debug)]
struct TabDrag {
    from: usize,
    grab: f32,
}

/// Which tabs the strip shows in `room` points: (first, how many, tab width). Tabs shrink to
/// `MIN_TAB`; past that a window of tabs around `selected` is shown and the rest go in the »
/// list.
pub fn tab_layout(n: usize, selected: usize, room: f32) -> (usize, usize, f32) {
    const MIN_TAB: f32 = 90.0;
    let n = n.max(1);
    let avail = (room - 34.0).max(MIN_TAB);
    if avail / n as f32 >= MIN_TAB {
        return (0, n, (avail / n as f32).min(220.0));
    }
    // Leave room for the » button.
    let fit = (((avail - 26.0) / MIN_TAB).floor() as usize).max(1);
    let first = selected.saturating_sub(fit - 1).min(n - fit);
    (first, fit, MIN_TAB)
}

/// The place among the shown tabs (`first..`, `widths` wide in strip order from `x0`) for tab
/// `from` whose left edge is dragged to `left`: the slot whose place for it is nearest.
pub fn drop_slot(left: f32, x0: f32, widths: &[f32], first: usize, from: usize) -> usize {
    let mut best = (0, f32::INFINITY);
    let mut at = x0;
    let others = widths.iter().enumerate().filter(|(k, _)| first + k != from).map(|(_, w)| *w);
    for (k, w) in others.map(Some).chain([None]).enumerate() {
        if (left - at).abs() < best.1 {
            best = (k, (left - at).abs());
        }
        at += w.unwrap_or(0.0);
    }
    first + best.0
}

/// A tab's hover text: where its design is (`place`), under its full name when the tab `cut` it
/// short.
pub fn tab_tip(name: &str, place: &str, cut: bool) -> String {
    if cut { format!("{name}\n{place}") } else { place.to_string() }
}

/// Tabs are rounded chips on the bar; the selected one has the stronger fill.
pub fn tab_shape(painter: &Painter, t: &Tokens, r: Rect, active: bool, hovered: bool) {
    let fill = if active {
        t.app_bar_tab_active
    } else if hovered {
        t.app_bar_hover
    } else {
        egui::Color32::TRANSPARENT
    };
    painter.rect_filled(r, t.radius_md, fill);
}

/// The selected tab's name field: Enter or leaving the field keeps the name, Escape gives it up.
fn rename_field(ui: &mut egui::Ui, rect: Rect, rename: &mut TabRename, handles: &TabHandles) -> Option<TabAction> {
    let t = Tokens::get();
    ui.painter().rect_filled(rect, t.radius_sm, t.field);
    let id = ui.scope_id().with("doc_tab_rename");
    let response = crate::inline_name(ui, rect.shrink2(vec2(4.0, 0.0)), id, rename.text);
    if rename.text.chars().count() > rename.max {
        *rename.text = rename.text.chars().take(rename.max).collect();
    }
    solidcraft_ui_registry::publish_target(handles.rename, rect);
    let was_focused = *rename.focused;
    if !*rename.focused {
        // The whole name selected, so typing replaces it.
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
            let end = rename.text.chars().count();
            state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(end))));
            state.store(ui.ctx(), id);
        }
        response.request_focus();
        *rename.focused = true;
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        Some(TabAction::RenameCancelled)
    } else if ui.input(|i| i.key_pressed(egui::Key::Enter)) || (was_focused && response.lost_focus()) {
        Some(TabAction::RenameDone)
    } else {
        None
    }
}

/// Draws the document tabs in `r` from `x0` to `x1`: click to select, × to close, drag to
/// reorder, a right-click for the tab's menu, » for every tab when they don't all fit, + for a
/// new one.
pub fn tabs(ui: &mut egui::Ui, strip: &mut TabStrip, r: Rect, x0: f32, x1: f32) -> Vec<TabAction> {
    let t = Tokens::get();
    let h = strip.handles;
    let n = strip.tabs.len();
    let (first, shown, w) = tab_layout(n, strip.selected, x1 - x0);
    let mut actions = Vec::new();
    // Each tab fits its name (up to MAX_FIT) while all of them fit; otherwise they share the room.
    const MAX_FIT: f32 = 200.0;
    // The selected tab grows with a name being typed into it.
    let typed = strip.rename.as_ref().map(|r| r.text.clone());
    let natural: Vec<f32> = strip
        .tabs
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            let shown_name = match &typed {
                Some(text) if i == strip.selected => text.clone(),
                _ => tab.name.to_string(),
            };
            let label = format!("{shown_name}{}", if tab.dirty { " •" } else { "" });
            let text = ui.painter().layout_no_wrap(label, crate::type_face::semibold(t.font_button), t.app_bar_text).size().x;
            let badge = if tab.security.is_some() { 18.0 } else { 0.0 };
            (text + 40.0 + badge).clamp(72.0, MAX_FIT)
        })
        .collect();
    let fit_all = shown == n && natural.iter().sum::<f32>() <= x1 - x0 - 34.0;
    let menu_id = strip.id.with("menu");
    let mut menu: Option<(usize, Pos2)> = ui.ctx().data(|d| d.get_temp(menu_id));
    // The shown tabs' widths, in strip order.
    let widths: Vec<f32> = (first..first + shown).map(|i| if fit_all { natural.get(i).copied().unwrap_or(w) } else { w }).collect();
    let width = |i: usize| i.checked_sub(first).and_then(|k| widths.get(k)).copied().unwrap_or(w);
    let strip_end = x0 + widths.iter().sum::<f32>();
    // A tab being dragged to a new place follows the pointer and the others make room for it;
    // Escape puts it back.
    let drag_id = strip.id.with("drag");
    let (held_down, released, escape) = ui.input(|i| (i.pointer.primary_down(), i.pointer.primary_released(), i.key_pressed(egui::Key::Escape)));
    let mut drag =
        ui.ctx().data(|d| d.get_temp::<TabDrag>(drag_id)).filter(|d| (held_down || released) && !escape && (first..first + shown).contains(&d.from));
    let pointer = ui.ctx().pointer_latest_pos();
    let held = drag.zip(pointer).map(|(d, p)| (d.from, (p.x - d.grab).clamp(x0, (strip_end - width(d.from)).max(x0))));
    let to = held.map(|(from, left)| (from, drop_slot(left, x0, &widths, first, from)));
    // Where each shown tab sits: in strip order, or with the dragged tab moved to its slot.
    let mut slots: Vec<usize> = (first..first + shown).filter(|&i| to.is_none_or(|(from, _)| from != i)).collect();
    if let Some((from, to)) = to {
        slots.insert(to.saturating_sub(first).min(slots.len()), from);
    }
    let mut lefts = vec![x0; shown];
    let mut at = x0;
    for &i in &slots {
        if let Some(l) = i.checked_sub(first).and_then(|k| lefts.get_mut(k)) {
            *l = at;
        }
        at += width(i);
    }
    // The dragged tab is drawn last, over the others.
    let mut order: Vec<usize> = (first..first + shown).filter(|&i| held.is_none_or(|(from, _)| from != i)).collect();
    order.extend(held.map(|(from, _)| from));
    let mut dropped: Option<(usize, usize)> = None;
    let (mut pick, mut shut) = (None, None);
    for i in order {
        let Some(tab) = strip.tabs.get(i).copied() else {
            continue;
        };
        let w = width(i);
        let left = match held {
            Some((from, left)) if from == i => left,
            _ => i.checked_sub(first).and_then(|k| lefts.get(k)).copied().unwrap_or(x0),
        };
        let tr = Rect::from_min_size(pos2(left, r.top() + 4.0), vec2(w - 2.0, r.height() - 8.0));
        let active = strip.shown_selected == Some(i);
        let resp = ui.interact(tr, ui.scope_id().with(("doc_tab", i)), Sense::click_and_drag());
        solidcraft_ui_registry::publish_target(&format!("{}{i}", h.tab), tr);
        // Dragging a tab along the strip moves it. The tabs are also part of the title bar:
        // dragging the only tab, or one mostly up or down, moves the window. A double click
        // renames the tab (the first click selected it), or maximizes the window as the bar's
        // empty space does.
        if resp.drag_started_by(egui::PointerButton::Primary) {
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(tr.center());
            let moved = pointer.unwrap_or(origin) - origin;
            if n > 1 && (!strip.title_bar || moved.x.abs() >= moved.y.abs()) {
                drag = Some(TabDrag { from: i, grab: origin.x - tr.left() });
            } else if strip.title_bar {
                crate::window_chrome::hand_press_to_os(ui.ctx(), egui::ViewportCommand::StartDrag);
            }
        } else if resp.double_clicked() {
            if strip.double_click_renames {
                if active {
                    actions.push(TabAction::StartRename { tab: i, rect: tr });
                }
            } else if strip.title_bar {
                actions.push(TabAction::Maximize);
            }
        }
        if resp.drag_stopped() && drag.is_some_and(|d| d.from == i) {
            // Released over the bar (not dropped somewhere down the window): the tab lands there.
            if let Some((from, to)) = to
                && pointer.is_some_and(|p| p.y < r.bottom() + r.height())
            {
                dropped = Some((from, to));
            }
            drag = None;
        }
        let lit = resp.hovered() || held.is_some_and(|(from, _)| from == i);
        tab_shape(ui.painter(), &t, tr, active, lit || resp.has_focus());
        let ink = if active || lit { t.app_bar_text } else { t.app_bar_text.gamma_multiply(0.7) };
        let label = format!("{}{}", tab.name, if tab.dirty { " •" } else { "" });
        let font = if active { crate::type_face::semibold(t.font_button) } else { FontId::proportional(t.font_button) };
        let g = ui.painter().layout_no_wrap(label, font, ink);
        let badge = if tab.security.is_some() { 18.0 } else { 0.0 };
        let text_w = (tr.width() - 30.0 - badge).max(10.0);
        let tip = tab_tip(tab.name, tab.place, g.size().x > text_w);
        let clip = Rect::from_min_size(tr.min, vec2(text_w + 10.0, tr.height()));
        match strip.rename.as_mut().filter(|_| active) {
            Some(rename) => {
                let field = Rect::from_min_max(pos2(tr.left() + 6.0, tr.top() + 3.0), pos2(tr.left() + 10.0 + text_w, tr.bottom() - 3.0));
                actions.extend(rename_field(ui, field, rename, &h));
            }
            None => {
                ui.painter().with_clip_rect(clip).galley(pos2(tr.left() + 10.0, tr.center().y - g.size().y / 2.0), g, ink);
            }
        }
        if let Some(badge) = tab.security {
            let br = Rect::from_center_size(pos2(tr.right() - 31.0, tr.center().y), vec2(14.0, 14.0));
            crate::icons::paint_ink(ui.painter(), br, "shield", badge.ink(&t));
        }
        // ×
        let xr = Rect::from_center_size(pos2(tr.right() - 13.0, tr.center().y), vec2(16.0, 16.0));
        let show_close = active || resp.hovered() || ui.rect_contains_pointer(tr);
        let xresp = ui.interact(xr, ui.scope_id().with(("doc_close", i)), if show_close { Sense::click() } else { Sense::hover() });
        solidcraft_ui_registry::publish_target(&format!("{}{i}", h.close), xr);
        if show_close && (xresp.hovered() || xresp.has_focus()) {
            ui.painter().rect_filled(xr, t.radius_sm.saturating_sub(1), t.app_bar_hover_strong);
        }
        if show_close {
            let opacity = if xresp.hovered() || xresp.has_focus() { 1.0 } else { 0.55 };
            crate::icons::paint_app_bar(ui.painter(), Rect::from_center_size(xr.center(), vec2(14.0, 14.0)), "close", opacity);
        }
        if resp.secondary_clicked() {
            menu = Some((i, ui.ctx().pointer_interact_pos().unwrap_or(tr.left_bottom())));
        }
        // Named apart from a dialog's Close: egui names an unnamed widget after its tooltip.
        xresp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, show_close, strip.close_name));
        // A middle click closes the tab, as in a web browser.
        if resp.middle_clicked() || xresp.middle_clicked() || xresp.backing_on_hover_text(strip.close_tip).clicked() {
            shut = Some(i);
        } else if (if held.is_some() { resp } else { resp.backing_on_hover_text(tip) }).clicked() {
            pick = Some(i);
        }
    }
    ui.ctx().data_mut(|d| match drag {
        Some(v) => {
            d.insert_temp(drag_id, v);
        }
        None => d.remove::<TabDrag>(drag_id),
    });
    // In the order they happened: a later choice this frame (the list's, the menu's) wins.
    actions.extend(shut.map(TabAction::Close));
    actions.extend(pick.map(TabAction::Select));
    let mut x = strip_end;
    // More tabs than fit: a chevron lists them all.
    if shown < n {
        let mr = Rect::from_center_size(pos2(x + 12.0, r.center().y), vec2(20.0, 22.0));
        let mresp = ui.interact(mr, ui.scope_id().with("doc_more"), Sense::click());
        solidcraft_ui_registry::publish_handle(h.more, mr.center());
        if mresp.hovered() {
            ui.painter().rect_filled(mr, t.radius_sm, t.app_bar_hover_button);
        }
        ui.painter().text(mr.center(), Align2::CENTER_CENTER, format!("»{}", n - shown), FontId::proportional(t.font_body), t.app_bar_text);
        let id = strip.id.with("more");
        if mresp.clone().backing_on_hover_text(strip.more_tip).clicked() {
            ui.ctx().data_mut(|d| d.insert_temp(id, !d.get_temp::<bool>(id).unwrap_or(false)));
        }
        if tab_list_open(ui.ctx(), strip.id) {
            let mut listed = None;
            let resp = crate::Container::new(ui.ctx(), crate::Surface::Popup)
                .area(id.with("area"))
                .fixed_pos(pos2(mr.left(), r.bottom()))
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    crate::menu::menu_frame(ui, |ui| {
                        for (i, tab) in strip.tabs.iter().enumerate() {
                            let label = format!("{}{}", tab.list_name, if tab.dirty { " •" } else { "" });
                            if crate::list_picker(ui, &format!("{}{i}", h.more_row), &label, i == strip.selected).clicked() {
                                listed = Some(i);
                            }
                        }
                    })
                });
            let pressed_outside = ui.ctx().input(|i| i.pointer.any_pressed()) && !resp.response.contains_pointer() && !mresp.contains_pointer();
            if listed.is_some() || pressed_outside {
                ui.ctx().data_mut(|d| d.insert_temp(id, false));
            }
            actions.extend(listed.map(TabAction::Select));
        }
        x += 26.0;
    }
    // The tab's menu.
    if let Some((i, at)) = menu.filter(|(i, _)| *i < n) {
        let resp = crate::Container::new(ui.ctx(), crate::Surface::Popup)
            .area(menu_id.with("area"))
            .fixed_pos(at)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                crate::menu::menu_frame(ui, |ui| {
                    let mut chosen = None;
                    for row in strip.menu {
                        let mut item = crate::Item::action(row.id, row.label, "");
                        if i == strip.selected && !row.key_on_selected.is_empty() {
                            item = item.key(row.key_on_selected);
                        }
                        item.enabled = !row.needs_others || n > 1;
                        let top = ui.cursor().top();
                        let picked = crate::menu::list_row(ui, &item);
                        let rect = Rect::from_min_max(pos2(ui.min_rect().left(), top), pos2(ui.min_rect().right(), ui.cursor().top()));
                        solidcraft_ui_registry::publish_handle(&format!("{}{}", h.menu_row, row.id), rect.center());
                        if picked.is_some() {
                            chosen = Some(row.id);
                        }
                    }
                    chosen
                })
            });
        if let Some(id) = resp.inner {
            actions.push(TabAction::Menu { tab: i, id: id.to_string() });
        }
        let pressed_outside = ui.ctx().input(|x| x.pointer.any_pressed()) && !resp.response.contains_pointer();
        menu = if resp.inner.is_some() || pressed_outside || ui.ctx().input(|x| x.key_pressed(egui::Key::Escape)) { None } else { Some((i, at)) };
    }
    ui.ctx().data_mut(|d| match menu {
        Some(m) => {
            d.insert_temp(menu_id, m);
        }
        None => {
            d.remove::<(usize, Pos2)>(menu_id);
        }
    });
    // A new tab.
    let pr = Rect::from_center_size(pos2(x + 14.0, r.center().y), vec2(22.0, 22.0));
    let presp = ui.interact(pr, ui.scope_id().with("doc_new"), Sense::click());
    if presp.hovered() {
        ui.painter().rect_filled(pr, t.radius_sm, t.app_bar_hover_button);
    }
    crate::icons::paint_app_bar(
        ui.painter(),
        Rect::from_center_size(pr.center(), vec2(16.0, 16.0)),
        "plus",
        if presp.hovered() { 1.0 } else { 0.55 },
    );
    solidcraft_ui_registry::publish_handle(h.new, pr.center());
    if presp.backing_on_hover_text(strip.new_tip).clicked() {
        actions.push(TabAction::New);
    }
    actions.extend(dropped.map(|(from, to)| TabAction::Reorder { from, to }));
    actions
}

/// A row of the File menu: what it runs (its `id`), its label, shortcut and kit icon.
#[derive(Clone, Debug, PartialEq)]
pub struct FileItem {
    pub id: String,
    pub label: String,
    pub key: String,
    pub icon: String,
    pub enabled: bool,
}

/// The File menu's rows, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum FileRow {
    /// A menu row with its shortcut.
    Item(FileItem),
    /// A plain button row, with an optional hover text.
    Button {
        item: FileItem,
        tip: Option<String>,
    },
    /// A submenu of rows (Open Recent).
    Sub {
        label: String,
        rows: Vec<FileRow>,
    },
    Separator,
}

/// The File menu drawn: the row chosen (its id), and whether the menu closes (a row was chosen or
/// a click landed elsewhere).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileMenuOut {
    pub chosen: Option<String>,
    pub close: bool,
}

fn file_rows(ui: &mut egui::Ui, rows: &[FileRow], chosen: &mut Option<String>, in_sub: bool) {
    for row in rows {
        match row {
            FileRow::Item(item) => {
                let row = crate::Item::action(&item.label, &item.label, &item.icon).key(&item.key).on_if(item.enabled);
                if crate::menu::list_row(ui, &row).is_some() {
                    *chosen = Some(item.id.clone());
                    if in_sub {
                        ui.close();
                    }
                }
            }
            FileRow::Button { item, tip } => {
                let response = crate::menu::button(ui, &item.label, &item.icon);
                let response = match tip {
                    Some(tip) => response.backing_on_hover_text(tip),
                    None => response,
                };
                if response.clicked() {
                    *chosen = Some(item.id.clone());
                    if in_sub {
                        ui.close();
                    }
                }
            }
            FileRow::Sub { label, rows } => {
                crate::Menu::button(ui, label, |ui| file_rows(ui, rows, chosen, true));
            }
            FileRow::Separator => {
                ui.separator();
            }
        }
    }
}

/// Draws the File menu's `rows` at `at` (under the File button) in the area `id`. `just_opened`:
/// the click that opened it this pass is not a click elsewhere. The rows scroll when the menu would
/// run past the window's bottom, so every row stays reachable in a short window.
pub fn file_menu(ctx: &egui::Context, rows: &[FileRow], at: Pos2, id: egui::Id, just_opened: bool) -> FileMenuOut {
    use crate::{Container, Surface};
    let mut chosen = None;
    // Room below `at` for the rows, less the menu frame's margins; never under three rows.
    let room = (ctx.content_rect().bottom() - at.y - FILE_MENU_BOTTOM_GAP).max(FILE_MENU_MIN_ROOM);
    let resp = Container::new(ctx, Surface::Popup).area(id).fixed_pos(at).order(egui::Order::Foreground).show(ctx, |ui| {
        crate::Menu::show_frame(ui, crate::Menu::frame(), |ui| {
            ui.set_min_width(190.0);
            ui.visuals_mut().widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
            ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::NONE;
            ui.visuals_mut().widgets.hovered.weak_bg_fill = Tokens::get().hover;
            // `min_scrolled_height` lets the rows grow past the size the area kept from an earlier,
            // shorter list, as the panel drop-downs do (#874); they still shrink to fit their content.
            egui::ScrollArea::vertical().id_salt("file_menu_rows").max_height(room).min_scrolled_height(room).show(ui, |ui| {
                file_rows(ui, rows, &mut chosen, false);
            });
        });
    });
    let close = chosen.is_some() || (!just_opened && resp.response.clicked_elsewhere());
    FileMenuOut { chosen, close }
}

/// The workspace switcher at the ribbon's left end: the workspace's icon and name over a caption
/// with a chevron (one line while the ribbon is folded, the stage bar's own look in a staged
/// workspace), under an accent strip.
pub struct Switcher<'a> {
    pub name: &'a str,
    pub caption: &'a str,
    /// A kit icon name.
    pub icon: &'a str,
    pub tip: &'a str,
    /// Its menu is open: drawn held.
    pub open: bool,
    /// On a floating card (rounder, its accent strip inset by the card's corner).
    pub floating: bool,
    /// The ribbon shows button labels (a taller switcher).
    pub labels: bool,
    /// The ribbon is folded to its tab row.
    pub folded: bool,
    /// The staged workspace's look.
    pub stage: bool,
    pub accent: egui::Color32,
    /// Its rect, and its rect as a target.
    pub handle_rect: &'a str,
    pub handle_target: &'a str,
}

/// Draws the switcher in `ws`; returns whether it was clicked (the app opens its workspace menu
/// from `ws`).
pub fn switcher(ui: &mut egui::Ui, data: &Switcher, ws: Rect) -> bool {
    let t = Tokens::get();
    let painter = ui.painter().clone();
    let wresp = ui.interact(ws, ui.scope_id().with("workspace_switcher"), Sense::click());
    if wresp.hovered() || data.open {
        painter.rect_filled(ws, if data.floating { t.radius_lg } else { 0 }, t.hover);
    }
    // Keep the accent straight while leaving the floating card's top corners round.
    let accent_inset = if data.stage {
        t.space_12
    } else if data.floating {
        f32::from(t.radius_lg)
    } else {
        0.0
    };
    let corner = if data.floating { t.radius_sm / 2 } else { 0 };
    painter.rect_filled(
        Rect::from_min_size(ws.left_top() + vec2(accent_inset, 0.0), vec2((ws.width() - 2.0 * accent_inset).max(0.0), t.space_3)),
        egui::CornerRadius { nw: 0, ne: 0, sw: corner, se: corner },
        data.accent,
    );
    if !data.floating {
        t.secondary_line(&painter, [ws.right_top(), ws.right_bottom()], t.border);
    }
    let caption_room = (ws.width() - 2.0 * t.space_8 - t.space_12).max(0.0);
    if data.stage {
        crate::StageBar::workspace(ui, ws, data.icon, data.name, data.caption);
    } else if data.folded {
        // Folded to the tab row: the workspace's name and its arrow, on one line.
        let galley = egui::WidgetText::from(egui::RichText::new(data.name).size(t.font_button).color(t.text)).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            caption_room,
            egui::TextStyle::Button,
        );
        let label_width = galley.size().x;
        crate::ribbon::galley_at(ui, pos2(ws.center().x - 6.0, ws.center().y + 1.0), galley, t.text);
        crate::ribbon::dropdown_arrow(&painter, pos2(ws.center().x - 6.0 + label_width / 2.0 + 7.0, ws.center().y + 1.5), t.text_dim);
    } else {
        let labels = data.labels;
        let icon = Rect::from_center_size(pos2(ws.center().x, ws.top() + if labels { 39.0 } else { 25.0 }), vec2(30.0, 30.0));
        crate::icons::paint_icon(&painter, icon, data.icon, 1.0);
        crate::ribbon::text(ui, pos2(ws.center().x, ws.top() + if labels { 69.0 } else { 50.0 }), data.name, t.font_button, t.text, true);
        // Long translations share the switcher's width with the arrow and its gap.
        let galley = egui::WidgetText::from(egui::RichText::new(data.caption).size(t.font_small).color(t.text_dim)).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            caption_room,
            egui::TextStyle::Button,
        );
        let label_width = galley.size().x;
        crate::ribbon::galley_at(ui, pos2(ws.center().x - 6.0, ws.top() + if labels { 89.0 } else { 70.0 }), galley, t.text_dim);
        let arrow = pos2(ws.center().x - 6.0 + label_width / 2.0 + 7.0, ws.top() + if labels { 89.5 } else { 70.5 });
        crate::ribbon::dropdown_arrow(&painter, arrow, t.text_dim);
    }
    let clicked = wresp.backing_on_hover_text(data.tip).clicked();
    solidcraft_ui_registry::publish_rect(data.handle_rect, ws);
    solidcraft_ui_registry::publish_target(data.handle_target, ws);
    clicked
}

#[cfg(test)]
#[path = "app_bar_tests.rs"]
mod tests;
