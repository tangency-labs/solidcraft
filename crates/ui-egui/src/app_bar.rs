//! The application bar as a component: the app's mark and name, Home, the File button, the file
//! actions (Save, Undo, Redo…), room for the app's quick-access buttons, and at the right Help,
//! Settings, Report a bug, the command search's place and the version. The app describes the bar
//! as plain data ([`AppBar`]); [`show`] draws it inside the title bar's content rect and returns
//! what was chosen ([`BarAction`]) with the places it leaves to the app ([`BarOut`]): its quick
//! access buttons, an offer pill, the document tabs, the assistant and the search field. Its parts
//! are published in the widget registry under the names the app gives ([`Handles`]).

use crate::ui_kit::BackingTooltipExt as _;
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
/// Space between the search field and Settings.
pub const SEARCH_GAP: f32 = 4.0;
/// A bar narrower than this shows no product name, no Undo and Redo, and narrower file actions.
const COMPACT: f32 = 640.0;

/// Where the bar publishes its parts in the widget registry.
#[derive(Clone, Copy, Debug)]
pub struct Handles<'a> {
    /// Home's centre under each of these names, and its rect under each of `home_rect`.
    pub home: &'a [&'a str],
    pub home_rect: &'a [&'a str],
    /// The File button, likewise.
    pub file: &'a [&'a str],
    pub file_rect: &'a [&'a str],
    /// A button's rect as a target, under each prefix followed by its id.
    pub button: &'a [&'a str],
    /// A separator's rect, this prefix followed by its group.
    pub separator: &'a str,
    pub search: &'a str,
    pub report: &'a str,
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
    /// The file actions after File.
    pub actions: Vec<BarButton<'a>>,
    /// How many quick-access buttons the app draws after them (one `step` each).
    pub quick: usize,
    pub help: BarButton<'a>,
    pub settings: BarButton<'a>,
    pub report: Report<'a>,
    /// The bar holds the command search field, just before Settings.
    pub search: bool,
    /// The bar keeps a place for the app's assistant button.
    pub assistant: bool,
    pub version: Option<Version<'a>>,
    pub handles: Handles<'a>,
}

/// What the user chose on the bar this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum BarAction {
    Home,
    /// The File button, at `rect`.
    File {
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
pub fn search_rect(ctx: &egui::Context, bar: Rect, right: f32, tabs_room: f32) -> Rect {
    let height = 26.0;
    let full = crate::ui_kit::CommandSearch::closed_width(ctx);
    let whole = bar.width() >= COMPACT && tabs_room - full - SEARCH_GAP >= 216.0;
    let width = if whole { full } else { crate::ui_kit::CommandSearch::compact_width(height) };
    Rect::from_min_size(pos2(right - width, bar.center().y - height / 2.0), vec2(width, height))
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
    let mut actions = Vec::new();
    (data.mark)(ui.painter(), pos2(left + 20.0, r.center().y));
    if !compact {
        ui.painter().text(
            pos2(left + 36.0, r.center().y),
            Align2::LEFT_CENTER,
            data.name,
            crate::ui_kit::type_face::semibold(t.font_heading),
            t.app_bar_text,
        );
    }
    let mut x = left + if compact { 56.0 } else { 128.0 };
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
    // The File menu's text button.
    let galley = ui.painter().layout_no_wrap(data.file.to_string(), FontId::proportional(t.font_button), t.app_bar_text);
    let file = Rect::from_min_size(pos2(x - 13.0, r.center().y - 13.0), vec2(galley.size().x + 16.0, 26.0));
    x = file.right() + 17.0;
    let resp = ui.interact(file, ui.scope_id().with(("ab", "file")), if idle { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, idle, data.file));
    for name in h.file {
        solidcraft_ui_registry::publish_handle(name, file.center());
    }
    for name in h.file_rect {
        solidcraft_ui_registry::publish_rect(name, file);
    }
    if idle && (resp.hovered() || resp.has_focus() || data.file_open) {
        ui.painter().rect_filled(file, t.radius_sm, t.app_bar_hover);
    }
    let ink = if idle { t.app_bar_text } else { t.app_bar_text.gamma_multiply(0.35) };
    ui.painter().galley(pos2(file.left() + 8.0, file.center().y - galley.size().y / 2.0), galley, ink);
    if idle && resp.clicked() {
        actions.push(BarAction::File { rect: file });
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
    // Settings' left edge; the search field, when the bar holds it, sits just before it and moves
    // the assistant and the end of the document tabs left by its width.
    let gear_left = report.left() - 68.0;
    let search = data.search.then(|| search_rect(ui.ctx(), r, gear_left - SEARCH_GAP, report.left() - CLUSTER_BUTTONS - tabs_start));
    let search_shift = search.map_or(0.0, |s| s.width() + SEARCH_GAP);
    if let Some(search) = search {
        solidcraft_ui_registry::publish_target(h.search, search);
    }
    if icon_button(ui, r, help_x(report), &data.help, compact, h) {
        actions.push(BarAction::Help { x: help_x(report) });
    }
    // An offer (a newer release) sits before the right cluster, in the space up to the file
    // actions; labelled where Report a bug is (an icon-only one leaves it to fit by room).
    let controls_left = report.left() - CLUSTER_BUTTONS - search_shift;
    let offer = Rect::from_min_max(pos2(x, report.top()), pos2(controls_left - 2.0, report.bottom()));
    let offer_labeled = report_label || (data.report.icon_only && r.width() >= COMPACT);
    // Report a bug.
    let response = ui.interact(report, ui.scope_id().with("sc_report_button"), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), data.report.label));
    solidcraft_ui_registry::publish_target(h.report, report);
    let hot = response.hovered() || response.has_focus();
    let tip = if data.report.icon_only {
        // Drawn like Settings and Help beside it.
        if hot {
            ui.painter().rect_filled(report.shrink(3.0), t.radius_sm, t.app_bar_hover_button);
        }
        crate::icons::paint_app_bar(ui.painter(), report.shrink(7.0), "bug_report", 1.0);
        data.report.label
    } else {
        ui.painter().rect_filled(report, t.radius_sm, if hot { t.app_bar_hover_button } else { t.app_bar });
        ui.painter().rect_stroke(report, t.radius_sm, Stroke::new(1.0, t.app_bar_separator), egui::StrokeKind::Inside);
        let icon = Rect::from_center_size(pos2(report.left() + 14.0, report.center().y), vec2(18.0, 18.0));
        crate::icons::paint_app_bar(ui.painter(), icon, "bug_report", 1.0);
        if report_label {
            ui.painter().text(
                pos2(report.left() + REPORT_TEXT_X, report.center().y),
                Align2::LEFT_CENTER,
                data.report.label,
                report_font,
                t.app_bar_text,
            );
        }
        data.report.tip
    };
    if response.backing_on_hover_text(tip).clicked() {
        actions.push(BarAction::Report);
    }
    // Settings sits immediately left of Help.
    let gear = Rect::from_center_size(pos2(report.left() - 52.0, r.center().y), vec2(32.0, 32.0));
    let gresp = ui.interact(gear, ui.scope_id().with("sc_prefs_button"), Sense::click());
    if gresp.hovered() || gresp.has_focus() {
        ui.painter().rect_filled(gear.shrink(3.0), t.radius_sm, t.app_bar_hover_button);
    }
    crate::icons::paint_app_bar(ui.painter(), gear.shrink(7.0), data.settings.icon, 1.0);
    solidcraft_ui_registry::publish_rect(h.settings, gear);
    let assistant = data.assistant.then(|| Rect::from_center_size(pos2(report.left() - 86.0 - search_shift, r.center().y), vec2(32.0, 32.0)));
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
    BarOut { actions, file, quick, offer, offer_labeled, tabs_start, controls_left, search, assistant }
}

#[cfg(test)]
#[path = "app_bar_tests.rs"]
mod tests;
