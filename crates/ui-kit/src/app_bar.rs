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
/// The least width of a document tab before the tabs scroll behind the » list.
const MIN_TAB: f32 = 90.0;
/// The expanded design tab (option A): its height and corner radius.
const DESIGN_PILL_HEIGHT: f32 = 30.0;
const DESIGN_GROUP_BACKING_HEIGHT: f32 = 27.0;
const PILL_RADIUS: u8 = 8;
/// The room a pill keeps right of its views: the +, a separator, the close button and its padding.
const PILL_AFTER: f32 = 63.0;
/// A view's sub-tab: its height, corner radius, label size, the gap between sub-tabs and the
/// longest label drawn before it is cut.
const VIEW_HEIGHT: f32 = 22.0;
const VIEW_RADIUS: u8 = 5;
const VIEW_FONT: f32 = 12.5;
const VIEW_GAP: f32 = 2.0;
/// The strip draws each tab this much narrower than its slot, so neighbours do not touch.
const TAB_MARGIN: f32 = 2.0;
const VIEW_LABEL_MAX: f32 = 180.0;
/// The least width a view's label takes while its name is typed.
const RENAME_MIN: f32 = 90.0;
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
/// The × inside a pill.
const PILL_X: f32 = 16.0;

/// Space between the search field and Settings.
pub const SEARCH_GAP: f32 = 4.0;
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
    /// Yields before primary actions when space is scarce.
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
    /// Saved movable items; None uses today's default arrangement.
    pub items: Option<&'a crate::app_bar_items::Layout>,
    /// Stable command ids corresponding to the host's drawable quick-access buttons.
    pub quick_ids: &'a [&'a str],
    /// The app's mark, painted centred at the point given, and its name beside it.
    pub mark: &'a dyn Fn(&Painter, Pos2),
    pub name: &'a str,
    /// Optional space for an app-owned control before Home.
    pub leading: Option<egui::Vec2>,
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

/// A menu's text button (File), in its laid-out rect: its rect, and whether it
/// was clicked while `idle`. Drawn held while its menu is `open`.
/// It shows `label`; `name` is its accessible name.
fn text_button(ui: &mut egui::Ui, rect: Rect, (label, name): (&str, &str), open: bool, idle: bool, salt: &str) -> (Rect, bool) {
    let t = Tokens::get();
    let galley = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(t.font_button), t.app_bar_text);
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
    /// An app-owned control, including a quick-access command by layout id.
    App {
        id: String,
    },
}

/// The bar drawn: what was chosen, and the places it leaves to the app.
#[derive(Debug)]
pub struct BarOut {
    /// Each quick button's source-list index and centre x, including reordered/cross-zone ones.
    pub quick_slots: Vec<(usize, f32)>,
    /// Space reserved for the app-owned control before Home.
    pub leading: Option<Rect>,
    pub actions: Vec<BarAction>,
    /// Hidden layout ids in bar order.
    pub overflow: Vec<String>,
    /// Core rows already have labels and actions; the host completes app-owned rows.
    pub overflow_items: Vec<OverflowItem>,
    pub overflow_button: Option<Rect>,
    /// Number of drawn quick-access buttons. Use `quick_slots` to identify them in a custom list.
    pub quick_count: usize,
    pub tabs_end: f32,
    pub managed: Option<Rect>,
    pub managed_labeled: bool,
    pub reserved_offers: bool,
    /// The File button, where its menu opens.
    pub file: Rect,
    /// Default arrangement's first quick-access centre and step; custom lists use `quick_slots`.
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

/// A flat capsule `PILL_HEIGHT` tall in `fill`, a touch lighter while `hot`.
fn capsule(ui: &egui::Ui, rect: Rect, fill: egui::Color32, hot: bool) {
    let fill = if hot { fill.lerp_to_gamma(egui::Color32::WHITE, 0.12) } else { fill };
    ui.painter().rect_filled(rect, egui::CornerRadius::same((rect.height() / 2.0) as u8), fill);
}

/// One pill in its laid-out rect. A flat capsule with full-height
/// corners; hovering lightens its fill by about 8 % over 0.1 s. The × is a plain glyph, with a
/// round translucent dark background (black at 38 %) that fades in while it is hovered. Edges are
/// snapped to whole pixels so they stay crisp at any scale.
fn pill(ui: &mut egui::Ui, bar: Rect, rect: Rect, p: &Pill, h: &Handles, actions: &mut Vec<BarAction>) {
    let t = Tokens::get();
    let ppp = ui.ctx().pixels_per_point();
    let snap = |v: f32| (v * ppp).round() / ppp;
    let rect = Rect::from_min_size(pos2(rect.left(), snap(bar.center().y - PILL_HEIGHT / 2.0)), vec2(rect.width(), PILL_HEIGHT));
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

/// Draws the app mark and optional title at the leading edge of a window's bar.
/// Returns the right edge of the brand so the window can place its next control.
pub fn paint_brand(painter: &Painter, content: Rect, mark: &dyn Fn(&Painter, Pos2), name: Option<&str>) -> f32 {
    let left = content.left();
    let y = content.center().y;
    mark(painter, pos2(left + 20.0, y));
    name.map_or(left + 32.0, |name| {
        let t = Tokens::get();
        painter.text(pos2(left + 36.0, y), Align2::LEFT_CENTER, name, crate::type_face::semibold(t.font_heading), t.app_bar_text).right()
    })
}

/// Draws the bar in `bar`, its controls within `content` (the title bar's room between the
/// window's own buttons).
pub fn show(ui: &mut egui::Ui, data: &AppBar, bar: Rect, content: Rect) -> BarOut {
    show_with_offers(ui, data, bar, content, Offers::default())
}

/// Measured app-owned transient controls, present from the first frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Offers {
    pub update: Option<f32>,
    pub managed: Option<f32>,
}

/// Draws the bar with explicitly measured transient controls.
pub fn show_with_offers(ui: &mut egui::Ui, data: &AppBar, bar: Rect, content: Rect, offers: Offers) -> BarOut {
    use crate::app_bar_layout::{application, layout};

    let t = Tokens::get();
    let h = &data.handles;
    let r = bar;
    let report_font = FontId::proportional(t.font_button);
    let label_width = ui.painter().layout_no_wrap(data.report.label.to_string(), report_font.clone(), t.app_bar_text).size().x;
    let version = data.version.map(|v| (v, ui.painter().layout_no_wrap(v.text.to_string(), FontId::proportional(t.font_small), t.app_bar_text)));
    let measured = application::Measurements {
        origin: r.left(),
        name: ui.painter().layout_no_wrap(data.name.to_string(), crate::type_face::semibold(t.font_heading), t.app_bar_text).size().x,
        file: ui.painter().layout_no_wrap(data.file.to_string(), FontId::proportional(t.font_button), t.app_bar_text).size().x + 16.0,
        leading: data.leading.map(|size| size.x),
        actions: data.actions.iter().map(|b| b.wide_only).collect(),
        quick: data.quick,
        report: label_width,
        report_icon_only: data.report.icon_only,
        pills: data.pills.iter().map(|p| pill_width(ui, p)).collect(),
        search: data.search.then(|| (crate::CommandSearch::closed_width(ui.ctx(), data.search_shortcut), crate::CommandSearch::compact_width(26.0))),
        assistant: data.assistant,
        version: version.as_ref().map(|(_, galley)| galley.size().x),
        offered: offers.update,
        managed: offers.managed,
        pixels_per_point: ui.ctx().pixels_per_point(),
    };
    let dimensions = (r.width(), content.left() - r.left(), r.right() - content.right());
    let spec = if let Some(items) = data.items {
        application::spec_with_items(
            dimensions,
            measured,
            items,
            &data.actions.iter().map(|b| b.id).collect::<Vec<_>>(),
            data.quick_ids,
            // The existing Ko-fi action/registry id stays intact; its saved placement is donate.
            &data.pills.iter().map(|p| if p.id == "kofi" { "donate" } else { p.id }).collect::<Vec<_>>(),
        )
    } else {
        application::spec(dimensions.0, dimensions.1, dimensions.2, measured)
    };
    let placed = layout(&spec);
    let geometry = placed.application.clone().unwrap_or_default();
    let visible = |id: &str| geometry.visible.iter().any(|s| s == id);
    let compact = geometry.compact;
    let rect = |span: application::Span, height| {
        Rect::from_min_max(pos2(r.left() + span.left, r.center().y - height / 2.0), pos2(r.left() + span.right, r.center().y + height / 2.0))
    };
    let mut actions = Vec::new();
    if visible("logo") {
        let brand = Rect::from_min_max(pos2(r.left() + geometry.mark - 20.0, r.top()), r.right_bottom());
        paint_brand(ui.painter(), brand, data.mark, geometry.name_visible.then_some(data.name));
    }
    let leading = geometry.leading.zip(data.leading).map(|(span, size)| rect(span, size.y));
    // Home.
    let home = rect(geometry.home, 32.0);
    let idle = data.home.enabled;
    if visible("home") {
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
    }
    if visible("brand") && geometry.brand_separator < geometry.file.left {
        separator(ui, r, r.left() + geometry.brand_separator, "brand", h);
    }
    // The File menu's text button.
    let file = rect(geometry.file, 26.0);
    let clicked = visible("file") && text_button(ui, file, (data.file, data.file), data.file_open, idle, "file").1;
    for name in h.file {
        solidcraft_ui_registry::publish_handle(name, file.center());
    }
    for name in h.file_rect {
        solidcraft_ui_registry::publish_rect(name, file);
    }
    if clicked {
        actions.push(BarAction::File { rect: file });
    }
    // The file actions, then the app's quick-access buttons.
    for (b, at) in data.actions.iter().zip(&geometry.actions) {
        if let Some(at) = at
            && icon_button(ui, r, r.left() + at, b, compact, h)
        {
            actions.push(BarAction::Button { id: b.id.to_string() });
        }
    }
    if let Some(at) = geometry.quick_separator {
        separator(ui, r, r.left() + at, "quick_access", h);
    }
    let quick = geometry.quick.map(|(at, step)| (r.left() + at, step));
    // The document tabs follow the file actions.
    if visible("documents") {
        separator(ui, r, r.left() + geometry.documents_separator, "documents", h);
    }
    let tabs_start = r.left() + geometry.tabs_start;
    let report = rect(geometry.report, if data.report.icon_only { 32.0 } else { 26.0 });
    let report_label = geometry.report_labeled;
    let search = geometry.search.map(|span| rect(span, 26.0));
    if let Some(search) = search {
        solidcraft_ui_registry::publish_target(h.search, search);
    }
    let help = r.left() + geometry.help;
    if visible("help") && icon_button(ui, r, help, &data.help, compact, h) {
        actions.push(BarAction::Help { x: help });
    }
    // An offer (a newer release) sits before the right cluster, in the space up to the file
    // actions; labelled where Report a bug is (an icon-only one leaves it to fit by room).
    let controls_left = r.left() + geometry.controls_left;
    let offer = rect(geometry.offer, report.height());
    let offer_labeled = geometry.offer_labeled;
    // Report a bug.
    if visible("report") {
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
    }
    // The link pills, left of the assistant.
    for (p, span) in data.pills.iter().zip(&geometry.pills).rev() {
        if span.right > span.left {
            let p = Pill { labelled: p.labelled && span.right - span.left > 45.0, ..*p };
            pill(ui, r, rect(*span, PILL_HEIGHT), &p, h, &mut actions);
        }
    }
    // Settings sits immediately left of Help.
    let gear = rect(geometry.settings, 32.0);
    if visible("settings") {
        let gresp = ui.interact(gear, ui.scope_id().with("sc_prefs_button"), Sense::click());
        if gresp.hovered() || gresp.has_focus() {
            ui.painter().rect_filled(gear.shrink(3.0), t.radius_sm, t.app_bar_hover_button);
        }
        crate::icons::paint_app_bar(ui.painter(), gear.shrink(7.0), data.settings.icon, 1.0);
        solidcraft_ui_registry::publish_rect(h.settings, gear);
        solidcraft_ui_registry::publish_handle(h.settings, gear.center());
        if gresp.backing_on_hover_text(data.settings.tip).clicked() {
            actions.push(BarAction::Button { id: data.settings.id.to_string() });
        }
    }
    let assistant = geometry.assistant.map(|span| rect(span, 32.0));
    if let Some((v, galley)) = version
        && let Some(span) = geometry.version
    {
        let vrect = rect(span, galley.size().y);
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
    let overflow_button = placed.overflow_button.map(|(x, w)| Rect::from_center_size(pos2(r.left() + x + w / 2.0, r.center().y), vec2(w, 32.0)));
    if let Some(button) = overflow_button {
        let more = solidcraft_ui_text::tr("ui.app_bar_more");
        let b = BarButton { id: "overflow", icon: "chevron", tip: more, enabled: true, narrow: false, wide_only: false };
        // Stable names independent of a host's handle prefixes.
        for handle in ["titlebar:overflow", "appbar:overflow"] {
            solidcraft_ui_registry::publish_target(handle, button);
        }
        if icon_button(ui, r, button.center().x, &b, false, h) {
            let pass = ui.ctx().cumulative_pass_nr();
            ui.ctx().data_mut(|d| {
                let id = egui::Id::unique("sc_appbar_overflow_open");
                let open = d.get_temp::<bool>(id).unwrap_or(false);
                d.insert_temp(id, !open);
                d.insert_temp(id.with("opened"), pass);
            });
        }
    }
    let overflow_items = overflow_items(data, &placed.overflow, overflow_button.map_or(r.left(), |b| b.left()));
    BarOut {
        quick_slots: geometry.quick_slots.iter().map(|&(i, x)| (i, r.left() + x)).collect(),
        leading,
        actions,
        file,
        quick,
        offer,
        offer_labeled,
        tabs_start,
        controls_left,
        search,
        assistant,
        overflow: placed.overflow,
        overflow_items,
        overflow_button,
        quick_count: geometry.quick_count,
        tabs_end: r.left() + geometry.tabs_end,
        managed: geometry.managed.map(|s| rect(s, 26.0)),
        managed_labeled: geometry.managed_labeled,
        reserved_offers: geometry.reserved_offers,
    }
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
    /// A view's sub-tab and its close button, this prefix followed by the view's place in its
    /// design; the + that opens another view; the "N more" button; the view switcher's button
    /// (its list's rows, and the "N more" list's, are this name and `view_more` plus `:row:`
    /// and the place).
    pub view: &'a str,
    pub view_close: &'a str,
    pub view_new: &'a str,
    pub view_more: &'a str,
    pub switcher: &'a str,
}

/// One document tab.
#[derive(Clone, Copy, Debug, Default)]
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
    /// The design's views in order. None or one: the tab is a plain tab (a one-view design gets a
    /// small + after its name). Several: the selected design's tab expands to show them (or a
    /// view switcher), and any other design's tab carries a count badge.
    pub views: &'a [ViewTab<'a>],
}

/// What a view's sub-tab says about its source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewState {
    #[default]
    Normal,
    /// The design a sheet draws from changed: a hollow dot.
    SourceChanged,
    /// The sheet was deleted (Undo restores it): a dashed outline.
    Deleted,
}

/// One view of a design as the strip shows it. The strip knows no kinds of view: the app gives
/// each its icon and colours.
#[derive(Clone, Copy, Debug)]
pub struct ViewTab<'a> {
    pub name: &'a str,
    /// What its icon draws: `model`, `sketch`, `sheet`, `render` or `animation` ([`paint_view_glyph`]).
    pub icon: &'a str,
    /// The icon's stroke colour.
    pub accent: egui::Color32,
    /// The colour of the shown view's underline.
    pub underline: egui::Color32,
    pub state: ViewState,
    /// The design shows this view.
    pub active: bool,
    /// A view listed here that belongs to another window.
    pub to_window: Option<u32>,
    /// Its hover text ("Sketch 2 · Design workspace · Ctrl+2").
    pub tip: &'a str,
}

/// Strings the view parts of the strip show, in the current language. `more` holds `{n}` for the
/// number of views it lists.
#[derive(Clone, Copy, Debug, Default)]
pub struct ViewText<'a> {
    pub close_view: &'a str,
    pub close_design: &'a str,
    pub new_view: &'a str,
    pub more: &'a str,
    pub switch_view: &'a str,
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
    /// This action belongs to the window that holds the view.
    pub needs_local_view: bool,
    /// A view glyph drawn before the label ([`paint_view_glyph`]); empty for none.
    pub icon: &'a str,
    /// The glyph's colour; none draws it in the menu's dim ink.
    pub accent: Option<egui::Color32>,
    /// A muted, non-clickable header (its `label` is the text) instead of a row.
    pub heading: bool,
    /// A separator line is drawn above this row.
    pub separator_before: bool,
    /// The rows of this item's flyout.
    pub children: &'a [TabMenuRow<'a>],
    pub checked: bool,
    pub disabled_tip: Option<&'a str>,
}

impl<'a> TabMenuRow<'a> {
    /// A plain row with no icon, always on, with no separator above it.
    pub const fn new(id: &'a str, label: &'a str, key_on_selected: &'a str) -> Self {
        Self {
            id,
            label,
            key_on_selected,
            needs_others: false,
            needs_local_view: false,
            icon: "",
            accent: None,
            heading: false,
            separator_before: false,
            children: &[],
            checked: false,
            disabled_tip: None,
        }
    }

    /// A muted header over the rows that follow; it is never chosen.
    pub const fn heading(label: &'a str) -> Self {
        Self { heading: true, ..Self::new(label, label, "") }
    }

    /// This row offered only while there are other tabs.
    pub const fn needing_others(mut self) -> Self {
        self.needs_others = true;
        self
    }

    /// Keep this action in the window that owns the view.
    pub const fn for_local_view(mut self) -> Self {
        self.needs_local_view = true;
        self
    }

    /// This row with a separator line above it.
    pub const fn separated(mut self) -> Self {
        self.separator_before = true;
        self
    }

    /// Opens these rows beside this row.
    pub const fn with_children(mut self, children: &'a [Self]) -> Self {
        self.children = children;
        self
    }

    /// This row with the view glyph `icon` in `accent` (the menu's dim ink for none).
    pub const fn with_icon(mut self, icon: &'a str, accent: Option<egui::Color32>) -> Self {
        self.icon = icon;
        self.accent = accent;
        self
    }
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
    /// Only this design's view controls appear in a pop-out's strip.
    pub pop_out: bool,
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
    /// The selected design's views are sub-tabs in its tab (option A); off, its tab has a view
    /// switcher (option B). Option A also falls back to B where the bar has no room for it.
    pub views_as_tabs: bool,
    /// The selected design's tab has a view switcher even with its one view (off: a design with one
    /// view is a plain tab). Ignored while `views_as_tabs` is on.
    pub switcher_always: bool,
    pub view_text: ViewText<'a>,
    /// The rows of a view's right-click menu, of the menu of the + and of the switcher's footer.
    pub view_menu: &'a [TabMenuRow<'a>],
    /// The id of the row of `view_menu` that closes the views to the right: off for the last view.
    pub view_right_row: &'a str,
    /// A view's name being edited in its sub-tab: its place in the design and the text.
    pub view_rename: Option<(usize, TabRename<'a>)>,
    pub plus_menu: &'a [TabMenuRow<'a>],
    /// The layouts the Layout button offers, each drawn as a miniature (its id ends in the layout's
    /// key: `single`, `side_by_side`, `stacked`, `large_and_two`, `grid`); empty: no button.
    pub layout_menu: &'a [TabMenuRow<'a>],
    /// The id of the layout shown now, outlined in the menu; empty for none.
    pub layout_current: &'a str,
    pub layout_tip: &'a str,
    /// The id of the choice of what an empty place of a layout shows (a row of `layout_menu`
    /// whose id starts with `fill.`, listed under the miniatures as a choice of one); empty for none.
    pub layout_fill: &'a str,
    /// The heading over those choices.
    pub layout_fill_heading: &'a str,
    pub switcher_menu: &'a [TabMenuRow<'a>],
    /// A design tab released this far below the bar is torn off ([`is_tear_off`]); 0 turns it off.
    pub tear_off_below: f32,
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
    /// The selected design's view at this place was chosen, or its close button was.
    SelectView(usize),
    CloseView(usize),
    /// The + was clicked and the strip has no menu for it.
    NewView,
    /// The name typed into view `view` of the selected design was kept, or the edit given up.
    ViewRenameDone {
        view: usize,
    },
    ViewRenameCancelled,
    /// A view dragged to a new place within its design.
    ReorderView {
        from: usize,
        to: usize,
    },
    /// The focused view tab was lifted with Space.
    LiftView {
        view: usize,
        origin: Rect,
    },
    /// A view tab being dragged. The app decides whether to dock, reorder or move it.
    DockView {
        view: usize,
        origin: Rect,
        pointer: Pos2,
        released: bool,
    },
    /// A row of a view's right-click menu, of the + menu, of the view switcher's footer.
    ViewMenu {
        view: usize,
        id: String,
    },
    PlusMenu {
        id: String,
    },
    /// A layout chosen from the Layout button's menu.
    LayoutMenu {
        id: String,
    },
    SwitcherMenu {
        id: String,
    },
    /// The design tab at this place was released well below the bar: it goes to a window of its own.
    TearOff(usize),
}

/// A tab dragged along the strip: the tab and where along it the pointer grabbed it.
#[derive(Clone, Copy, Debug)]
struct TabDrag {
    from: usize,
    grab: f32,
}

/// A view dragged along its design's sub-tabs: its place and where along it the pointer grabbed it.
#[derive(Clone, Copy, Debug)]
struct ViewDrag {
    from: usize,
    grab: f32,
    origin: Rect,
    cancelled: bool,
}

/// How the selected design's tab is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectedLayout {
    /// A plain tab (a design with one view unless the switcher is asked for always, or none).
    Plain,
    /// Expanded into a pill with its views as sub-tabs.
    Pill,
    /// One tab with a view switcher.
    Switcher,
}

/// How the selected design's tab is drawn: `views` views, the app asks for sub-tabs (`as_tabs`)
/// or for a switcher even with one view (`switcher_always`, ignored with `as_tabs`), the pill needs
/// `pill_min` points at the least and the bar leaves it `room`.
pub fn selected_layout(views: usize, as_tabs: bool, switcher_always: bool, pill_min: f32, room: f32) -> SelectedLayout {
    if views == 1 && switcher_always && !as_tabs {
        SelectedLayout::Switcher
    } else if views < 2 {
        SelectedLayout::Plain
    } else if as_tabs && pill_min <= room {
        SelectedLayout::Pill
    } else {
        SelectedLayout::Switcher
    }
}

/// Which sub-tabs a pill shows in `room` points: (first, how many). `widths` are the sub-tabs' own
/// widths. All of them when they fit; otherwise a run of neighbours that holds `active` (the
/// shown view never goes under the "N more" button, which takes `more` points).
pub fn view_window(widths: &[f32], active: usize, room: f32, more: f32) -> (usize, usize) {
    let n = widths.len();
    let used = |from: usize, to: usize| widths.get(from..to).map_or(0.0, |run| run.iter().sum::<f32>() + VIEW_GAP * run.len() as f32);
    if n == 0 || used(0, n) <= room + 0.5 {
        return (0, n);
    }
    let active = active.min(n.saturating_sub(1));
    for first in 0..=active {
        let mut end = first;
        while end < n && used(first, end + 1) <= room - more {
            end += 1;
        }
        if end > active {
            return (first, end - first);
        }
    }
    (active, 1)
}

/// Whether a design tab released with the pointer at `pointer_y` is torn off: more than `below`
/// under the bar's bottom (`below` 0: never).
pub fn is_tear_off(pointer_y: f32, bar_bottom: f32, below: f32) -> bool {
    below > 0.0 && pointer_y > bar_bottom + below
}

/// Where the left edge of the sub-tab `from` lands when dropped at `slot` (a place in
/// [`drop_slot`]'s numbering): the run of the others' `widths` (`first` is the place of the first),
/// from `x0`, up to the slot.
pub fn slot_left(x0: f32, widths: &[f32], first: usize, from: usize, slot: usize) -> f32 {
    let before = widths.iter().enumerate().filter(|(k, _)| first + k != from).take(slot.saturating_sub(first)).map(|(_, w)| *w);
    x0 + before.sum::<f32>()
}

/// Which tabs the strip shows in `room` points: (first, how many, tab width). Tabs shrink to
/// `MIN_TAB`; past that a window of tabs around `selected` is shown and the rest go in the »
/// list.
pub fn tab_layout(n: usize, selected: usize, room: f32) -> (usize, usize, f32) {
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

/// What a small + inside a one-view design's tab takes, and a count badge's font and a view
/// switcher's "/" width.
const SMALL_PLUS: f32 = 22.0;
const BADGE_FONT: f32 = 10.5;
const SLASH: f32 = 7.0;

/// The menus the selected design's tab opens, where (the point its menu starts at), from what
/// (the button that opened it: a press on it is not a press elsewhere) and in which frame (a press
/// in that frame is the one that opened it, not one outside).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Popup {
    View(usize),
    Plus,
    Layout,
    Switcher,
    More,
}
type PopupState = Option<(Popup, Pos2, Rect, u64)>;

/// What drawing the selected design's views reports back, and what they keep between frames.
struct Pen<'a> {
    actions: Vec<TabAction>,
    /// The view whose name is being edited (its place), and the text.
    rename: Option<(usize, TabRename<'a>)>,
    popup: PopupState,
    drag: Option<ViewDrag>,
    /// The design's close button was clicked.
    close_design: bool,
    /// The bar's bottom, where the menus open.
    below: f32,
    view_centres: Vec<(usize, f32)>,
}

/// How the selected design's tab is laid out: what [`measure`] decided.
struct Geom {
    kind: SelectedLayout,
    /// The design's place, and its tab's width.
    index: usize,
    width: f32,
    /// The room of the unsaved dot, and of the name (and the shield beside it).
    dot: f32,
    name_w: f32,
    /// Each view's sub-tab width, the "N more" button's and the switcher button's.
    view_ws: Vec<f32>,
    more_w: f32,
    switch_w: f32,
}

fn text_width(ui: &egui::Ui, text: &str, font: FontId) -> f32 {
    ui.painter().layout_no_wrap(text.to_string(), font, Tokens::get().app_bar_text).size().x
}

fn view_font(active: bool) -> FontId {
    if active { crate::type_face::semibold(VIEW_FONT) } else { FontId::proportional(VIEW_FONT) }
}

/// The width of a count badge showing `n`.
fn badge_width(ui: &egui::Ui, n: usize) -> f32 {
    (text_width(ui, &n.to_string(), crate::type_face::semibold(BADGE_FONT)) + 8.0).max(16.0)
}

/// The width of a view's sub-tab with a label `label` wide: its icon, label, the dot of a changed
/// source and, for the shown view, its close button.
fn view_width(label: f32, view: &ViewTab) -> f32 {
    let dot = if view.state == ViewState::SourceChanged { 14.0 } else { 0.0 };
    let mark = if view.to_window.is_some() { WINDOW_MARK_WIDTH } else { 0.0 };
    9.0 + 15.0 + 6.0 + label + dot + mark + if view.active || view.to_window.is_some() { 6.0 + 20.0 } else { 10.0 }
}

const WINDOW_MARK_WIDTH: f32 = 17.0;
const DETACHED_VIEW_OPACITY: f32 = 0.62;
const DESIGN_CLOSE_WIDTH: f32 = 33.0;

/// What the Layout button takes in the pill, beside its plus.
const LAYOUT_BUTTON: f32 = 28.0;

/// What a pill takes after its views: the + (and the Layout button), the separator and the close.
fn pill_after(strip: &TabStrip) -> f32 {
    PILL_AFTER - if strip.pop_out { DESIGN_CLOSE_WIDTH } else { 0.0 } + if strip.layout_menu.is_empty() { 0.0 } else { LAYOUT_BUTTON }
}

/// What a pill takes before its views: border, padding, the unsaved dot, the name and its separator.
fn pill_before(dot: f32, name_w: f32) -> f32 {
    13.0 + dot + name_w + 10.0 + 1.0 + 8.0 + VIEW_GAP
}

fn more_label(strip: &TabStrip, hidden: usize) -> String {
    strip.view_text.more.replace("{n}", &hidden.to_string())
}

/// Whether tab `i` is the shown, selected design with a single view while views are sub-tabs: its
/// tab carries a small + after its name. With a switcher the tab stays as it was before views.
fn small_plus(strip: &TabStrip, i: usize) -> bool {
    strip.views_as_tabs && i == strip.selected && strip.shown_selected == Some(i) && strip.tabs.get(i).is_some_and(|tab| tab.views.len() == 1)
}

fn line_colour(t: &Tokens) -> egui::Color32 {
    t.app_bar.lerp_to_gamma(t.app_bar_separator, 0.4)
}

fn outline(r: Rect) -> [Pos2; 5] {
    [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()]
}

/// Lays out the selected design's tab for its views, `room` points of the bar being free for it.
fn measure(ui: &egui::Ui, strip: &TabStrip, tab: &DocTab, name: &str, view_typed: Option<(usize, &str)>, room: f32) -> Geom {
    let t = Tokens::get();
    let n = tab.views.len();
    let dot = if tab.dirty { 14.0 } else { 0.0 };
    let name_w = text_width(ui, name, crate::type_face::semibold(t.font_button)) + if tab.security.is_some() { 18.0 } else { 0.0 };
    let label_w = |k: usize, v: &ViewTab| match view_typed {
        Some((i, text)) if i == k => text_width(ui, text, view_font(true)).max(RENAME_MIN),
        _ => text_width(ui, v.name, view_font(v.active)),
    };
    let view_ws: Vec<f32> = tab.views.iter().enumerate().map(|(k, v)| view_width(label_w(k, v).min(VIEW_LABEL_MAX), v)).collect();
    let more_w = 8.0 + text_width(ui, &more_label(strip, n), FontId::proportional(VIEW_FONT)) + 3.0 + 13.0 + 6.0;
    let active = tab.views.iter().position(|v| v.active).unwrap_or(0);
    let active_w = view_ws.get(active).copied().unwrap_or(0.0);
    let before = pill_before(dot, name_w);
    let after = pill_after(strip);
    let pill = before + view_ws.iter().map(|w| w + VIEW_GAP).sum::<f32>() + after + TAB_MARGIN;
    let pill_min = before + active_w + VIEW_GAP + more_w + VIEW_GAP + after + TAB_MARGIN;
    let kind = if strip.pop_out { SelectedLayout::Pill } else { selected_layout(n, strip.views_as_tabs, strip.switcher_always, pill_min, room) };
    let kind = if view_typed.is_some() && kind == SelectedLayout::Plain { SelectedLayout::Switcher } else { kind };
    let shown = tab.views.get(active).map_or("", |v| v.name);
    let switch_w = 8.0 + 15.0 + 6.0 + text_width(ui, shown, view_font(true)).min(VIEW_LABEL_MAX) + 6.0 + badge_width(ui, n) + 6.0 + 13.0 + 4.0;
    let switcher = 13.0 + dot + name_w + 8.0 + SLASH + 8.0 + switch_w + 8.0 + 22.0 + 4.0;
    let incoming = ui.ctx().data(|data| {
        let lifted_here = data.get_temp::<ViewDrag>(strip.id.with("view_drag")).is_some_and(|drag| !drag.cancelled);
        if lifted_here { 0.0 } else { data.get_temp::<(usize, f32)>(strip.id.with("view_drag_slot")).map_or(0.0, |(_, width)| width + VIEW_GAP) }
    });
    let width = if kind == SelectedLayout::Pill { (pill + incoming).min(room) } else { switcher.min(room.max(MIN_TAB)) };
    let (name_w, switch_w) = if kind == SelectedLayout::Switcher {
        // The tab's painted frame is two points narrower than its allocation. Keep the
        // switcher and close targets inside it before giving the design name any room.
        let available = (width - 2.0 - (13.0 + dot + 8.0 + SLASH + 8.0 + 8.0 + 22.0 + 4.0)).max(0.0);
        let name_w = name_w.min((available - switch_w.min(72.0)).max(0.0));
        (name_w, switch_w.min((available - name_w).max(0.0)))
    } else {
        (name_w, switch_w)
    };
    Geom { kind, index: strip.selected, width, dot, name_w, view_ws, more_w, switch_w }
}

/// Opens the menu `which` at `at`, or closes it when it is the one open.
fn toggle(ctx: &egui::Context, popup: &mut PopupState, which: Popup, at: Pos2, opener: Rect) {
    *popup = if popup.is_some_and(|(open, ..)| open == which) { None } else { Some((which, at, opener, ctx.cumulative_frame_nr())) };
}

/// A count badge starting at `at.x`, centred on `at.y`; `strong` on a lit ground. Its rect.
fn count_badge(ui: &egui::Ui, at: Pos2, n: usize, strong: bool) -> Rect {
    let t = Tokens::get();
    let rect = Rect::from_min_size(pos2(at.x, at.y - 8.0), vec2(badge_width(ui, n), 16.0));
    ui.painter().rect_filled(rect, 8u8, if strong { t.app_bar_hover_strong } else { t.app_bar_hover });
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, n.to_string(), crate::type_face::semibold(BADGE_FONT), t.app_bar_text);
    rect
}

fn chevron_down(p: &Painter, c: Pos2, size: f32, ink: egui::Color32) {
    crate::disclosure::dropdown_indicator(p, Rect::from_center_size(c, vec2(size, size)), ink);
}

/// The strokes of a view icon in a 24 by 24 box.
fn glyph_lines(icon: &str) -> &'static [&'static [(f32, f32)]] {
    match icon {
        "model" => &[
            &[(12.0, 3.0), (20.0, 7.0), (20.0, 17.0), (12.0, 21.0), (4.0, 17.0), (4.0, 7.0), (12.0, 3.0)],
            &[(4.0, 7.0), (12.0, 11.0), (20.0, 7.0)],
            &[(12.0, 11.0), (12.0, 21.0)],
        ],
        "sketch" => &[&[(4.0, 20.0), (5.0, 16.0), (16.0, 5.0), (19.0, 8.0), (8.0, 19.0), (4.0, 20.0)], &[(14.0, 7.0), (17.0, 10.0)]],
        "sheet" => &[
            &[(6.0, 3.0), (15.0, 3.0), (19.0, 7.0), (19.0, 21.0), (6.0, 21.0), (6.0, 3.0)],
            &[(15.0, 3.0), (15.0, 7.0), (19.0, 7.0)],
            &[(9.0, 12.0), (16.0, 12.0)],
            &[(9.0, 16.0), (16.0, 16.0)],
        ],
        "render" => &[&[(8.0, 9.0), (8.9, 7.9), (9.9, 7.2), (11.0, 7.0)]],
        "animation" => &[&[(5.0, 5.0), (19.0, 5.0), (19.0, 19.0), (5.0, 19.0), (5.0, 5.0)], &[(10.0, 9.0), (15.0, 12.0), (10.0, 15.0), (10.0, 9.0)]],
        "rename" => &[&[(4.0, 20.0), (5.0, 16.0), (16.0, 5.0), (19.0, 8.0), (8.0, 19.0), (4.0, 20.0)]],
        "window" => &[
            &[(14.0, 4.0), (20.0, 4.0), (20.0, 10.0)],
            &[(20.0, 4.0), (11.0, 13.0)],
            &[(18.0, 14.0), (18.0, 20.0), (4.0, 20.0), (4.0, 6.0), (10.0, 6.0)],
        ],
        "close" => &[&[(6.0, 6.0), (18.0, 18.0)], &[(18.0, 6.0), (6.0, 18.0)]],
        "duplicate" => &[&[(8.0, 8.0), (19.0, 8.0), (19.0, 19.0), (8.0, 19.0), (8.0, 8.0)], &[(5.0, 15.0), (5.0, 5.0), (15.0, 5.0)]],
        "tiles" => &[&[(4.0, 4.0), (20.0, 4.0), (20.0, 20.0), (4.0, 20.0), (4.0, 4.0)], &[(12.0, 4.0), (12.0, 20.0)], &[(4.0, 12.0), (20.0, 12.0)]],
        "reopen" => &[
            &[(3.0, 12.0), (4.2, 16.5), (7.5, 19.8), (12.0, 21.0), (16.5, 19.8), (19.8, 16.5), (21.0, 12.0), (19.8, 7.5), (16.5, 4.2), (12.0, 3.0)],
            &[(3.0, 4.0), (3.0, 9.0), (8.0, 9.0)],
        ],
        _ => &[],
    }
}

/// Draws a view's icon (`model`, `sketch`, `sheet`, `render` or `animation`, or the menu glyphs
/// `rename`, `window`, `close`, `duplicate`, `tiles` and `reopen`; another name draws nothing) as strokes of `ink` in `r`.
pub fn paint_view_glyph(p: &Painter, r: Rect, icon: &str, ink: egui::Color32) {
    let s = r.width() / 24.0;
    let stroke = Stroke::new(2.0 * s, ink);
    let at = |(x, y): (f32, f32)| r.min + vec2(x * s, y * s);
    if icon == "render" {
        p.circle_stroke(at((12.0, 12.0)), 9.0 * s, stroke);
    }
    for line in glyph_lines(icon) {
        p.add(egui::Shape::line(line.iter().map(|&point| at(point)).collect(), stroke));
    }
}

/// The rows of a menu: the id of the one chosen. Keys show with `keys`, rows that need others are
/// off without `others`, and the row `off` is off; each row's centre is published under `prefix` and its id.
fn menu_rows<'a>(
    ui: &mut egui::Ui,
    rows: &'a [TabMenuRow<'a>],
    keys: bool,
    others: bool,
    off: &str,
    prefix: &str,
    mut flyout: Option<&mut TabFlyout<'a>>,
) -> Option<String> {
    let mut chosen = None;
    let local_view = flyout.as_ref().is_none_or(|state| state.local_view);
    for (i, row) in rows.iter().filter(|row| !row.needs_local_view || local_view).enumerate() {
        if row.separator_before && i > 0 {
            crate::menu::list_row(ui, &crate::Item::sep());
        }
        if row.heading {
            crate::menu::list_row(ui, &crate::Item::heading(row.label));
            continue;
        }
        let mut item = crate::Item::action(row.id, row.label, "");
        if !row.icon.is_empty() {
            let (icon, ink) = (row.icon.to_string(), row.accent.unwrap_or(Tokens::get().form_dim));
            item.icon = crate::ItemIcon::paint(move |p, r, opacity| paint_view_glyph(p, r, &icon, ink.gamma_multiply(opacity)));
        }
        if keys && !row.key_on_selected.is_empty() {
            item = item.key(row.key_on_selected);
        }
        item.enabled = (!row.needs_others || others) && row.id != off && row.disabled_tip.is_none();
        let mut control = crate::MenuItem::from_item(&item, flyout.as_ref().is_some_and(|f| f.open == Some(i)));
        control.checked = row.checked.then_some(true);
        control.submenu = !row.children.is_empty();
        let response = control.show(ui);
        if let Some(tip) = row.disabled_tip {
            response.clone().backing_on_hover_text(tip);
        }
        solidcraft_ui_registry::publish_target(&format!("{prefix}{}", row.id), response.rect);
        if let Some(flyout) = flyout.as_deref_mut() {
            if control.submenu {
                let keyboard = response.has_focus()
                    && ui.input(|input| [egui::Key::ArrowRight, egui::Key::Enter, egui::Key::Space].iter().any(|&key| input.key_pressed(key)));
                if item.enabled && (response.hovered() || response.clicked() || keyboard) {
                    flyout.open = Some(i);
                    flyout.focus_first |= keyboard;
                }
                if flyout.open == Some(i) && item.enabled {
                    flyout.target = Some((response.rect, row.children));
                }
                continue;
            }
            if flyout.focus_first && item.enabled && flyout.open.is_none() && flyout.target.is_none() {
                response.request_focus();
                flyout.focus_first = false;
            }
            if response.hovered() {
                flyout.open = None;
                flyout.target = None;
            }
        }
        if response.clicked() && item.enabled {
            chosen = Some(row.id.to_string());
        }
    }
    chosen
}

/// The open flyout row and its anchor in the parent menu.
struct TabFlyout<'a> {
    open: Option<usize>,
    target: Option<(Rect, &'a [TabMenuRow<'a>])>,
    focus_first: bool,
    local_view: bool,
}

/// Every view as a menu row (a check mark on the shown one, a hollow dot on one whose source
/// changed, a dashed outline on a deleted one): the place of the one chosen. Rows are published
/// under `prefix` and their place.
fn view_list(ui: &mut egui::Ui, views: &[ViewTab], prefix: &str, menu: &mut Option<(usize, Rect)>) -> Option<usize> {
    let t = Tokens::get();
    let mut chosen = None;
    for (k, view) in views.iter().enumerate() {
        let id = format!("{prefix}{k}");
        let glyph = |p: &Painter, r: Rect, opacity: f32| paint_view_glyph(p, r, view.icon, view.accent.gamma_multiply(opacity));
        let mut row = crate::MenuItem::new(&id, view.name, crate::Icon::Paint(&glyph));
        row.checked = Some(view.active);
        let resp = if view.to_window.is_some() {
            ui.scope(|ui| {
                ui.set_opacity(DETACHED_VIEW_OPACITY);
                row.show(ui)
            })
            .inner
            .backing_on_hover_text(solidcraft_ui_text::tr("multi_window.window_mark"))
        } else {
            row.show(ui)
        };
        solidcraft_ui_registry::publish_target(&id, resp.rect);
        if view.to_window.is_some() {
            let mark = Rect::from_center_size(pos2(resp.rect.right() - t.space_16, resp.rect.center().y), vec2(13.0, 13.0));
            paint_view_glyph(ui.painter(), mark, "window", t.form_ink.gamma_multiply(DETACHED_VIEW_OPACITY));
            solidcraft_ui_registry::publish_target(&format!("{id}:window"), mark);
        }
        match view.state {
            ViewState::SourceChanged => {
                // Left of the check mark that closes the row of the shown view.
                ui.painter().circle_stroke(pos2(resp.rect.right() - 32.0, resp.rect.center().y), 3.25, Stroke::new(1.5, t.warning));
            }
            ViewState::Deleted => {
                let dash = Stroke::new(1.0, t.app_bar_separator.gamma_multiply(0.7));
                ui.painter().extend(crate::dashed_line(&outline(resp.rect.shrink(1.0)), resp.rect, dash, 3.0, 2.0));
            }
            ViewState::Normal => {}
        }
        if resp.secondary_clicked() {
            *menu = Some((k, resp.rect));
        } else if resp.clicked() {
            chosen = Some(k);
        }
    }
    chosen
}

/// The + that opens another view, in `rect` with a glyph `icon` points wide: a click opens the +
/// menu (or asks for a new view where the app gave no menu), a right click opens the menu.
fn plus_button(ui: &mut egui::Ui, strip: &TabStrip, rect: Rect, icon: f32, pen: &mut Pen) {
    let t = Tokens::get();
    let resp = ui.interact(rect, ui.scope_id().with("view_plus"), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, strip.view_text.new_view));
    solidcraft_ui_registry::publish_target(strip.handles.view_new, rect);
    let hot = resp.hovered() || resp.has_focus();
    if hot {
        ui.painter().rect_filled(rect, VIEW_RADIUS, t.app_bar_hover_button);
    }
    crate::icons::paint_app_bar(ui.painter(), Rect::from_center_size(rect.center(), vec2(icon, icon)), "plus", if hot { 1.0 } else { 0.7 });
    let resp = resp.backing_on_hover_text(strip.view_text.new_view);
    let at = pos2(rect.left(), pen.below);
    if resp.secondary_clicked() {
        pen.popup = Some((Popup::Plus, at, rect, ui.ctx().cumulative_frame_nr()));
    } else if resp.clicked() {
        if strip.plus_menu.is_empty() {
            pen.actions.push(TabAction::NewView);
        } else {
            toggle(ui.ctx(), &mut pen.popup, Popup::Plus, at, rect);
        }
    }
}

/// The close button of the selected design's expanded tab, in `rect` with a glyph `icon` points wide.
fn close_design_button(ui: &mut egui::Ui, strip: &TabStrip, tab: &DocTab, g: &Geom, rect: Rect, icon: f32, pen: &mut Pen) {
    let t = Tokens::get();
    let name = format!("{} {}", strip.view_text.close_design, tab.name);
    let resp = ui.interact(rect, ui.scope_id().with("view_close_design"), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.as_str()));
    solidcraft_ui_registry::publish_target(&format!("{}{}", strip.handles.close, g.index), rect);
    let hot = resp.hovered() || resp.has_focus();
    if hot {
        ui.painter().rect_filled(rect, 4u8, t.app_bar_hover_strong);
    }
    crate::icons::paint_app_bar(ui.painter(), Rect::from_center_size(rect.center(), vec2(icon, icon)), "close", if hot { 1.0 } else { 0.7 });
    pen.close_design |= resp.backing_on_hover_text(name).clicked();
}

/// The "N more" button after the views a pill has room for, in `rect`.
fn more_button(ui: &mut egui::Ui, strip: &TabStrip, rect: Rect, hidden: usize, pen: &mut Pen) {
    let t = Tokens::get();
    let label = more_label(strip, hidden);
    let resp = ui.interact(rect, ui.scope_id().with("view_more"), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label.as_str()));
    solidcraft_ui_registry::publish_target(strip.handles.view_more, rect);
    let open = pen.popup.is_some_and(|(which, ..)| which == Popup::More);
    if resp.hovered() || resp.has_focus() || open {
        ui.painter().rect_filled(rect, VIEW_RADIUS, t.app_bar_hover);
    }
    let ink = t.app_bar_text.gamma_multiply(0.85);
    let galley = ui.painter().layout_no_wrap(label, FontId::proportional(VIEW_FONT), ink);
    ui.painter().galley(pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0), galley, ink);
    chevron_down(ui.painter(), pos2(rect.right() - 12.5, rect.center().y), 13.0, ink);
    if resp.clicked() {
        toggle(ui.ctx(), &mut pen.popup, Popup::More, pos2(rect.left(), pen.below), rect);
    }
}

/// Starts a drag while the primary press still belongs to this tab. Touch needs a dwell
/// while a mouse uses a movement threshold.
fn start_view_drag(ui: &egui::Ui, response: &egui::Response, tab: Rect, from: usize) -> Option<ViewDrag> {
    if !response.is_pointer_button_down_on() {
        return None;
    }
    const MOUSE_TRAVEL: f32 = 4.0;
    const TOUCH_HOLD: f64 = 0.250;
    let (origin, dwell) = ui.input(|input| {
        if !input.pointer.primary_down() || input.key_pressed(egui::Key::Escape) {
            return (None, None);
        }
        let Some(origin) = input.pointer.press_origin() else { return (None, None) };
        if input.any_touches() {
            let elapsed = input.pointer.press_start_time().map_or(0.0, |started| input.time - started);
            return if elapsed >= TOUCH_HOLD { (Some(origin), None) } else { (None, Some(TOUCH_HOLD - elapsed)) };
        }
        let moved = input.pointer.interact_pos().is_some_and(|point| origin.distance_sq(point) >= MOUSE_TRAVEL * MOUSE_TRAVEL);
        (moved.then_some(origin), None)
    });
    if let Some(delay) = dwell.and_then(|dwell| std::time::Duration::try_from_secs_f64(dwell).ok()) {
        ui.ctx().request_repaint_after(delay);
    }
    origin.map(|point| ViewDrag { from, grab: point.x - tab.left(), origin: tab, cancelled: false })
}

/// One view's sub-tab in `vr`: icon, label, the shown view's underline and close button; a click
/// shows it, a middle click closes it, a right click opens its menu, a drag starts a move.
fn draw_view(ui: &mut egui::Ui, strip: &TabStrip, k: usize, view: &ViewTab, vr: Rect, pen: &mut Pen) {
    let t = Tokens::get();
    let h = strip.handles;
    let painter = ui.painter().clone();
    let detached = view.to_window.is_some();
    let close = view.active || detached;
    let opacity = if detached { DETACHED_VIEW_OPACITY } else { 1.0 };
    let main = if close { Rect::from_min_max(vr.min, pos2(vr.right() - 20.0, vr.bottom())) } else { vr };
    let resp = ui.interact(main, ui.scope_id().with(("doc_view", k)), Sense::click_and_drag());
    solidcraft_ui_registry::publish_target(&format!("{}{k}", h.view), vr);
    resp.widget_info(|| {
        let name = if detached {
            format!("{} — {}", view.name, solidcraft_ui_text::tr("multi_window.window_mark")).into()
        } else {
            std::borrow::Cow::Borrowed(view.name)
        };
        egui::WidgetInfo::selected(egui::Role::Tab, true, view.active, name)
    });
    if !detached && pen.drag.is_none() && !pen.rename.as_ref().is_some_and(|(index, _)| *index == k) {
        pen.drag = start_view_drag(ui, &resp, vr, k);
    }
    let dragged = pen.drag.is_some_and(|drag| drag.from == k);
    if dragged && pen.drag.is_some_and(|drag| !drag.cancelled) {
        let dash = Stroke::new(1.0, t.accent);
        painter.extend(crate::dashed_line(&outline(vr.shrink(0.5)), vr.expand(2.0), dash, 3.0, 2.0));
        return;
    }
    let resp = resp.backing_on_hover_text(view.tip);
    let deleted = view.state == ViewState::Deleted;
    if view.active {
        painter.rect_filled(vr, VIEW_RADIUS, t.app_bar_hover_strong);
    } else if resp.hovered() {
        painter.rect_filled(vr, VIEW_RADIUS, t.app_bar_hover);
    }
    if deleted {
        let dash = Stroke::new(1.0, t.app_bar_separator.gamma_multiply(0.7));
        painter.extend(crate::dashed_line(&outline(vr.shrink(0.5)), vr.expand(2.0), dash, 3.0, 2.0));
    }
    if resp.has_focus() {
        painter.rect_stroke(vr, VIEW_RADIUS, Stroke::new(2.0, t.accent), egui::StrokeKind::Inside);
    }
    let dim = t.app_bar_text.gamma_multiply(0.55);
    let ink = if deleted {
        dim
    } else if view.active || detached {
        t.app_bar_text
    } else {
        t.app_bar_text.gamma_multiply(0.8)
    }
    .gamma_multiply(opacity);
    paint_view_glyph(
        &painter,
        Rect::from_center_size(pos2(vr.left() + 16.5, vr.center().y), vec2(15.0, 15.0)),
        view.icon,
        (if deleted { dim } else { view.accent }).gamma_multiply(opacity),
    );
    // A view being renamed shows its name field in place of the label, and nothing else of it acts.
    if let Some(rename) = pen.rename.as_mut().filter(|(i, _)| *i == k).map(|(_, r)| r) {
        let field = Rect::from_min_max(pos2(vr.left() + 26.0, vr.top() + 2.0), pos2(vr.right() - if close { 26.0 } else { 4.0 }, vr.bottom() - 2.0));
        let done = rename_field(ui, field, rename, &h)
            .map(|a| if a == TabAction::RenameDone { TabAction::ViewRenameDone { view: k } } else { TabAction::ViewRenameCancelled });
        pen.actions.extend(done);
        return;
    }
    let galley = painter.layout_no_wrap(view.name.to_string(), view_font(view.active), ink);
    let label_x = vr.left() + 30.0;
    let label_w = galley.size().x.min(VIEW_LABEL_MAX);
    let label_y = vr.center().y - galley.size().y / 2.0;
    painter.with_clip_rect(Rect::from_min_size(pos2(label_x, vr.top()), vec2(label_w, VIEW_HEIGHT))).galley(pos2(label_x, label_y), galley, ink);
    if deleted {
        painter.hline(egui::Rangef::new(label_x, label_x + label_w), vr.center().y, Stroke::new(1.0, ink));
    }
    if view.state == ViewState::SourceChanged {
        painter.circle_stroke(pos2(label_x + label_w + 10.0, vr.center().y), 3.25, Stroke::new(1.5, t.warning.gamma_multiply(opacity)));
    }
    if detached {
        let dot_width = if view.state == ViewState::SourceChanged { 14.0 } else { 0.0 };
        let rect = Rect::from_center_size(pos2(label_x + label_w + dot_width + 10.0, vr.center().y), vec2(13.0, 13.0));
        paint_view_glyph(&painter, rect, "window", ink);
        solidcraft_ui_registry::publish_target(&format!("{}{k}:window", h.view), rect);
    }
    if view.active {
        let underline = Rect::from_min_max(pos2(vr.left() + 7.0, vr.bottom() - 2.0), pos2(vr.right() - 7.0, vr.bottom()));
        painter.rect_filled(underline, 2u8, view.underline);
    }
    if close {
        let xr = Rect::from_center_size(pos2(vr.right() - 10.0, vr.center().y), vec2(20.0, 20.0));
        let xresp = ui.interact(xr, ui.scope_id().with(("doc_view_close", k)), Sense::click());
        let name = format!("{} {}", strip.view_text.close_view, view.name);
        xresp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.as_str()));
        solidcraft_ui_registry::publish_target(&format!("{}{k}", h.view_close), xr);
        let hot = xresp.hovered() || xresp.has_focus();
        if hot {
            painter.rect_filled(xr, 4u8, t.app_bar_hover_button);
        }
        crate::icons::paint_app_bar(&painter, Rect::from_center_size(xr.center(), vec2(11.0, 11.0)), "close", if hot { 1.0 } else { 0.7 * opacity });
        if xresp.backing_on_hover_text(name).clicked() {
            pen.actions.push(TabAction::CloseView(k));
            return;
        }
    }
    if !detached
        && !dragged
        && resp.has_focus()
        && ui.input_mut(|i| i.modifiers == egui::Modifiers::NONE && i.consume_key(egui::Modifiers::NONE, egui::Key::Space))
    {
        pen.actions.push(TabAction::LiftView { view: k, origin: vr });
    } else if resp.middle_clicked() {
        pen.actions.push(TabAction::CloseView(k));
    } else if resp.secondary_clicked() {
        pen.popup = Some((Popup::View(k), pos2(vr.left(), pen.below), vr, ui.ctx().cumulative_frame_nr()));
    } else if resp.clicked() && !dragged {
        resp.request_focus();
        pen.actions.push(TabAction::SelectView(k));
    }
}

fn group_backing(rect: Rect) -> Rect {
    rect.shrink2(vec2(0.0, (rect.height() - DESIGN_GROUP_BACKING_HEIGHT).max(0.0) * 0.5))
}

/// The selected design's tab as a pill (option A) in `pr`: its frame and separator, the views that
/// fit as sub-tabs (the rest under "N more"), the +, and the close button. The name block (the
/// design's name, which the caller draws) is the left part; a view dragged along the sub-tabs
/// shows where it lands and is reported when released.
fn draw_pill(ui: &mut egui::Ui, strip: &TabStrip, tab: &DocTab, pr: Rect, g: &Geom, pen: &mut Pen) {
    let t = Tokens::get();
    let painter = ui.painter().clone();
    let line = Stroke::new(1.0, line_colour(&t));
    let cy = pr.center().y;
    let backing = group_backing(pr);
    painter.rect_filled(backing, PILL_RADIUS, t.app_bar_tab_active);
    painter.rect_stroke(backing, PILL_RADIUS, line, egui::StrokeKind::Inside);
    let separator = |x: f32| painter.line_segment([pos2(x, cy - 9.0), pos2(x, cy + 9.0)], line);
    let name_end = pr.left() + 13.0 + g.dot + g.name_w + 10.0;
    separator(name_end + 0.5);
    let close = Rect::from_center_size(pos2(pr.right() - 17.0, cy), vec2(24.0, 24.0));
    if !strip.pop_out {
        separator(close.left() - 4.0);
    }
    let has_layout = !strip.layout_menu.is_empty();
    let layout_x = close.left() - 20.0 + if strip.pop_out { DESIGN_CLOSE_WIDTH } else { 0.0 };
    let layout = Rect::from_center_size(pos2(layout_x, cy), vec2(24.0, 24.0));
    let plus = if has_layout { layout.translate(vec2(-LAYOUT_BUTTON, 0.0)) } else { layout };
    let start = name_end + 1.0 + 8.0 + VIEW_GAP;
    let active = tab.views.iter().position(|v| v.active).unwrap_or(0);
    let right = (pr.right() - pill_after(strip)).max(start);
    let (first, count) = view_window(&g.view_ws, active, right - start, g.more_w + VIEW_GAP);
    let more = (count < tab.views.len()).then_some((right - g.more_w).max(start));
    let right = more.map_or(right, |left| (left - VIEW_GAP).max(start));
    let slot = ui.ctx().data(|data| data.get_temp::<(usize, f32)>(strip.id.with("view_drag_slot")));
    let source = pen.drag.filter(|drag| !drag.cancelled);
    let gap = slot.map(|(index, width)| {
        let width = width.min((right - start).max(0.0));
        let mut x = start;
        let mut local = tab.views.iter().take(first).filter(|view| view.to_window.is_none()).count();
        for (k, view) in tab.views.iter().enumerate().skip(first).take(count) {
            if local >= index {
                break;
            }
            if source.is_none_or(|drag| drag.from != k) {
                x += g.view_ws[k] + VIEW_GAP;
            }
            if view.to_window.is_none() {
                local += 1;
            }
        }
        (index, Rect::from_min_size(pos2(x.min(right - width), cy - VIEW_HEIGHT * 0.5), vec2(width, VIEW_HEIGHT)))
    });
    ui.ctx().data_mut(|data| {
        let id = strip.id.with("view_drag_gap");
        if let Some(gap) = gap {
            data.insert_temp(id, gap);
        } else {
            data.remove::<(usize, Rect)>(id);
        }
    });
    let slide_time = if slot.is_some() && ui.ctx().global_style().animation_time > 0.0 { 0.14 } else { 0.0 };
    let clip = ui.clip_rect();
    ui.set_clip_rect(clip.intersect(Rect::from_min_max(pos2(start, pr.top()), pos2(right, pr.bottom()))));
    let mut x = start;
    let mut local = tab.views.iter().take(first).filter(|view| view.to_window.is_none()).count();
    for (k, view) in tab.views.iter().enumerate().skip(first).take(count) {
        let w = g.view_ws.get(k).copied().unwrap_or(0.0);
        let rect = Rect::from_min_size(pos2(x, cy - VIEW_HEIGHT / 2.0), vec2(w, VIEW_HEIGHT));
        let opening_after = gap.is_some_and(|(index, _)| local >= index);
        let open = gap.filter(|_| opening_after).map_or(0.0, |(_, rect)| rect.width() + VIEW_GAP);
        let close = source.filter(|drag| slot.is_some() && k > drag.from).map_or(0.0, |drag| drag.origin.width() + VIEW_GAP);
        let offset = ui.ctx().animate_value_with_time(strip.id.with(("view_slide", k)), open - close, slide_time);
        if view.to_window.is_none() {
            // A moving neighbour cannot move the zone that chose the move.
            pen.view_centres.push((local, rect.center().x));
            local += 1;
        }
        let rect = rect.translate(vec2(offset, 0.0));
        if gap.is_none() || source.is_none_or(|drag| drag.from != k) {
            let region = ui.clip_rect();
            if let Some((_, gap)) = gap {
                let mut side = region;
                if opening_after {
                    side.min.x = side.min.x.max(gap.right());
                } else {
                    side.max.x = side.max.x.min(gap.left());
                }
                ui.set_clip_rect(side);
            }
            if ui.clip_rect().is_positive() {
                draw_view(ui, strip, k, view, rect, pen);
            }
            ui.set_clip_rect(region);
        }
        x += w + VIEW_GAP;
    }
    ui.set_clip_rect(clip);
    if let Some(x) = more {
        more_button(ui, strip, Rect::from_min_size(pos2(x, cy - VIEW_HEIGHT / 2.0), vec2(g.more_w, VIEW_HEIGHT)), tab.views.len() - count, pen);
    }
    let pointer = ui.ctx().pointer_interact_pos();
    let released = ui.input(|i| i.pointer.primary_released());
    let viewport = ui.ctx().viewport_rect();
    if let Some((drag, pointer)) = pen.drag.filter(|drag| !drag.cancelled).zip(pointer) {
        pen.actions.push(TabAction::DockView { view: drag.from, origin: drag.origin, pointer, released });
        if released && (first..first + count).contains(&drag.from) && pointer.y <= pen.below + 6.0 && viewport.contains(pointer) {
            let widths: Vec<f32> = (first..first + count).map(|k| g.view_ws.get(k).copied().unwrap_or(0.0) + VIEW_GAP).collect();
            let slot = drop_slot(pointer.x - drag.grab, start, &widths, first, drag.from);
            if slot != drag.from {
                pen.actions.push(TabAction::ReorderView { from: drag.from, to: slot });
            }
        }
    }
    if released {
        pen.drag = None;
    }
    plus_button(ui, strip, plus, 14.0, pen);
    if has_layout {
        layout_button(ui, strip, layout, pen);
    }
    if !strip.pop_out {
        close_design_button(ui, strip, tab, g, close, 12.0, pen);
    }
}

/// The Layout button next to the +: always there, it opens the miniatures of the ways to arrange
/// the design's views in the window.
fn layout_button(ui: &mut egui::Ui, strip: &TabStrip, rect: Rect, pen: &mut Pen) {
    let t = Tokens::get();
    let resp = ui.interact(rect, ui.scope_id().with("view_layout"), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, strip.layout_tip));
    let open = pen.popup.is_some_and(|(which, ..)| which == Popup::Layout);
    let hot = resp.hovered() || resp.has_focus() || open;
    if hot {
        ui.painter().rect_filled(rect, VIEW_RADIUS, t.app_bar_hover_button);
    }
    let glyph = Rect::from_center_size(rect.center(), vec2(15.0, 15.0));
    paint_view_glyph(ui.painter(), glyph, "tiles", t.app_bar_text.gamma_multiply(if hot { 1.0 } else { 0.75 }));
    let resp = resp.backing_on_hover_text(strip.layout_tip);
    if resp.clicked() {
        toggle(ui.ctx(), &mut pen.popup, Popup::Layout, pos2(rect.left(), pen.below), rect);
    }
}

/// The key of a layout row's id (`layout.grid` is `grid`).
fn layout_key(id: &str) -> &str {
    id.rsplit('.').next().unwrap_or(id)
}

/// Paints the miniature of the layout `key` in `rect`: the focused view is the tinted cell and
/// the others are framed.
fn paint_layout(p: &Painter, rect: Rect, key: &str) {
    let t = Tokens::get();
    let inner = rect.shrink(6.0);
    let gap = 3.0;
    let (w, h) = (inner.width(), inner.height());
    let half = |len: f32| (len - gap) / 2.0;
    let cell = |x: f32, y: f32, cw: f32, ch: f32| Rect::from_min_size(inner.min + vec2(x, y), vec2(cw, ch));
    let cells: Vec<Rect> = match key {
        "side_by_side" => vec![cell(0.0, 0.0, half(w), h), cell(half(w) + gap, 0.0, half(w), h)],
        "stacked" => vec![cell(0.0, 0.0, w, half(h)), cell(0.0, half(h) + gap, w, half(h))],
        "large_and_two" => {
            let big = w * 0.6;
            vec![cell(0.0, 0.0, big, h), cell(big + gap, 0.0, w - big - gap, half(h)), cell(big + gap, half(h) + gap, w - big - gap, half(h))]
        }
        "grid" => vec![
            cell(0.0, 0.0, half(w), half(h)),
            cell(half(w) + gap, 0.0, half(w), half(h)),
            cell(0.0, half(h) + gap, half(w), half(h)),
            cell(half(w) + gap, half(h) + gap, half(w), half(h)),
        ],
        _ => vec![cell(0.0, 0.0, w, h)],
    };
    for (i, c) in cells.into_iter().enumerate() {
        if i == 0 {
            p.rect_filled(c, 3u8, t.accent_soft);
        } else {
            p.rect_filled(c, 3u8, t.panel_header);
            p.rect_stroke(c, 3u8, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        }
    }
}

/// The Layout menu: a miniature and a name for each layout, the current one outlined. The id of
/// the one chosen.
fn layout_grid(ui: &mut egui::Ui, strip: &TabStrip) -> Option<String> {
    const COLUMNS: usize = 3;
    const THUMB: egui::Vec2 = vec2(88.0, 56.0);
    let t = Tokens::get();
    let mut chosen = None;
    crate::menu::list_row(ui, &crate::Item::heading(strip.layout_tip));
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.set_max_width((THUMB.x + 30.0) * COLUMNS as f32);
        ui.spacing_mut().item_spacing = vec2(6.0, 8.0);
        for row in strip.layout_menu.iter().filter(|row| !row.heading && !row.id.starts_with("fill.")) {
            let (rect, resp) = ui.allocate_exact_size(vec2(THUMB.x + 24.0, THUMB.y + 20.0), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, solidcraft_ui_text::tr_label(row.label)));
            let thumb = Rect::from_min_size(rect.min + vec2(12.0, 0.0), THUMB);
            let current = row.id == strip.layout_current;
            if resp.hovered() {
                ui.painter().rect_filled(thumb.expand(2.0), 6u8, t.app_bar_hover_button);
            }
            ui.painter().rect_filled(thumb, 6u8, t.dialog_body);
            let ring = if current { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, t.border) };
            ui.painter().rect_stroke(thumb, 6u8, ring, egui::StrokeKind::Inside);
            paint_layout(ui.painter(), thumb, layout_key(row.id));
            ui.painter().text(
                pos2(rect.center().x, thumb.bottom() + 11.0),
                Align2::CENTER_CENTER,
                solidcraft_ui_text::tr_label(row.label),
                if current { crate::type_face::semibold(VIEW_FONT) } else { FontId::proportional(VIEW_FONT) },
                t.text,
            );
            if resp.clicked() {
                chosen = Some(row.id.to_string());
            }
        }
    });
    let fills: Vec<_> = strip.layout_menu.iter().filter(|row| row.id.starts_with("fill.")).collect();
    if !fills.is_empty() {
        ui.add_space(6.0);
        ui.separator();
        crate::menu::list_row(ui, &crate::Item::heading(strip.layout_fill_heading));
        for row in fills {
            let (rect, resp) = ui.allocate_exact_size(vec2((THUMB.x + 30.0) * COLUMNS as f32, 24.0), Sense::click());
            let label = solidcraft_ui_text::tr_label(row.label);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::RadioButton, true, label));
            if resp.hovered() {
                ui.painter().rect_filled(rect, 5u8, t.app_bar_hover_button);
            }
            let dot = pos2(rect.left() + 12.0, rect.center().y);
            ui.painter().circle_stroke(dot, 6.0, Stroke::new(1.5, if row.id == strip.layout_fill { t.accent } else { t.text_dim }));
            if row.id == strip.layout_fill {
                ui.painter().circle_filled(dot, 3.0, t.accent);
            }
            ui.painter().text(pos2(rect.left() + 26.0, rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(VIEW_FONT), t.text);
            if resp.clicked() {
                chosen = Some(row.id.to_string());
            }
        }
    }
    chosen
}

/// The selected design's tab as one tab with a view switcher (option B) in `pr`: the name, a "/",
/// a button with the shown view's icon, name and the number of views (it opens the list of every
/// view and the footer rows), and the close button.
fn draw_switcher(ui: &mut egui::Ui, strip: &TabStrip, tab: &DocTab, pr: Rect, g: &Geom, pen: &mut Pen) {
    let t = Tokens::get();
    let painter = ui.painter().clone();
    let cy = pr.center().y;
    painter.rect_filled(group_backing(pr), t.radius_md, t.app_bar_tab_active);
    let slash_x = pr.left() + 13.0 + g.dot + g.name_w + 8.0;
    painter.text(pos2(slash_x, cy), Align2::LEFT_CENTER, "/", FontId::proportional(t.font_button), t.app_bar_text.gamma_multiply(0.5));
    let shown =
        pen.rename.as_ref().and_then(|(index, _)| tab.views.get(*index)).or_else(|| tab.views.iter().find(|v| v.active)).or(tab.views.first());
    if let Some(view) = shown {
        let btn = Rect::from_min_size(pos2(slash_x + SLASH + 8.0, cy - VIEW_HEIGHT / 2.0), vec2(g.switch_w, VIEW_HEIGHT));
        let resp = ui.interact(btn, ui.scope_id().with("view_switcher"), Sense::click());
        let name = format!("{}: {}. {}", tab.name, view.name, strip.view_text.switch_view);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.as_str()));
        solidcraft_ui_registry::publish_target(strip.handles.switcher, btn);
        let open = pen.popup.is_some_and(|(which, ..)| which == Popup::Switcher);
        painter.rect_filled(btn, VIEW_RADIUS, if resp.hovered() || open { t.app_bar_hover_strong } else { t.app_bar_hover });
        if resp.has_focus() {
            painter.rect_stroke(btn.expand(1.0), VIEW_RADIUS + 1, Stroke::new(2.0, t.accent), egui::StrokeKind::Outside);
        }
        if let Some((index, rename)) = pen.rename.as_mut() {
            let field = btn.shrink(2.0);
            let done = rename_field(ui, field, rename, &strip.handles).map(|action| {
                if action == TabAction::RenameDone { TabAction::ViewRenameDone { view: *index } } else { TabAction::ViewRenameCancelled }
            });
            pen.actions.extend(done);
        } else {
            let clip = ui.clip_rect();
            ui.set_clip_rect(clip.intersect(btn));
            let painter = painter.with_clip_rect(ui.clip_rect());
            paint_view_glyph(&painter, Rect::from_center_size(pos2(btn.left() + 15.5, cy), vec2(15.0, 15.0)), view.icon, view.accent);
            let galley = painter.layout_no_wrap(view.name.to_string(), view_font(true), t.app_bar_text);
            let label_x = btn.left() + 29.0;
            let label_w = galley.size().x.min(VIEW_LABEL_MAX).min((btn.width() - 29.0 - badge_width(ui, tab.views.len()) - 29.0).max(0.0));
            let label_y = cy - galley.size().y / 2.0;
            painter.with_clip_rect(Rect::from_min_size(pos2(label_x, btn.top()), vec2(label_w, VIEW_HEIGHT)).intersect(btn)).galley(
                pos2(label_x, label_y),
                galley,
                t.app_bar_text,
            );
            let badge = count_badge(ui, pos2(label_x + label_w + 6.0, cy), tab.views.len(), true);
            chevron_down(&painter, pos2(badge.right() + 12.5, cy), 13.0, t.app_bar_text.gamma_multiply(0.85));
            if resp.backing_on_hover_text(strip.view_text.switch_view).clicked() {
                toggle(ui.ctx(), &mut pen.popup, Popup::Switcher, pos2(btn.left(), pen.below), btn);
            }
            ui.set_clip_rect(clip);
        }
    }
    close_design_button(ui, strip, tab, g, Rect::from_center_size(pos2(pr.right() - 15.0, cy), vec2(22.0, 22.0)), 12.0, pen);
}

/// The open menu of the selected design's views (a view's, the +'s, the switcher's, the "N more"
/// list): draws it and returns what was chosen through `pen`. It closes when a row is chosen, on
/// Escape and on a press elsewhere.
fn view_popups(ui: &mut egui::Ui, strip: &TabStrip, pen: &mut Pen) {
    let Some((which, at, opener, opened)) = pen.popup else {
        return;
    };
    let views = strip.tabs.get(strip.selected).map_or(&[][..], |tab| tab.views);
    if matches!(which, Popup::View(k) if k >= views.len()) {
        pen.popup = None;
        return;
    }
    let h = strip.handles;
    let mut next_menu = None;
    let flyout_id = strip.id.with("tab_flyout");
    let open = ui.ctx().data(|d| d.get_temp::<(Popup, u64, usize)>(flyout_id)).filter(|state| state.0 == which && state.1 == opened);
    let local_view = match which {
        Popup::View(index) => views.get(index).is_none_or(|view| view.to_window.is_none()),
        _ => true,
    };
    let mut flyout = TabFlyout { open: open.map(|state| state.2), target: None, focus_first: false, local_view };
    let resp = crate::Container::new(ui.ctx(), crate::Surface::Popup)
        .area(strip.id.with("view_popup_area"))
        .fixed_pos(at)
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            crate::menu::menu_frame(ui, |ui| match which {
                Popup::View(k) => {
                    let locals = views.iter().filter(|view| view.to_window.is_none()).count();
                    let off = if views.iter().skip(k + 1).any(|view| view.to_window.is_none()) { "" } else { strip.view_right_row };
                    menu_rows(ui, strip.view_menu, true, locals > 1, off, h.menu_row, Some(&mut flyout)).map(|id| TabAction::ViewMenu { view: k, id })
                }
                Popup::Plus => menu_rows(ui, strip.plus_menu, true, true, "", h.menu_row, None).map(|id| TabAction::PlusMenu { id }),
                Popup::Layout => layout_grid(ui, strip).map(|id| TabAction::LayoutMenu { id }),
                Popup::More => view_list(ui, views, &format!("{}:row:", h.view_more), &mut next_menu).map(TabAction::SelectView),
                Popup::Switcher => {
                    let picked = view_list(ui, views, &format!("{}:row:", h.switcher), &mut next_menu).map(TabAction::SelectView);
                    crate::menu::Menu::separator(ui);
                    let footer =
                        menu_rows(ui, strip.switcher_menu, true, true, "", h.menu_row, Some(&mut flyout)).map(|id| TabAction::SwitcherMenu { id });
                    picked.or(footer)
                }
            })
        });
    let sub = flyout.target.map(|(anchor, rows)| {
        let mut submenu = TabFlyout { open: None, target: None, focus_first: flyout.focus_first, local_view };
        crate::menu::Menu::submenu(ui.ctx(), flyout_id.with("area"), anchor, 40.0, false, |ui| {
            menu_rows(ui, rows, false, true, "", h.menu_row, Some(&mut submenu))
        })
    });
    let in_submenu = sub.as_ref().is_some_and(|menu| menu.response.contains_pointer());
    let selected = resp.inner.or_else(|| {
        let id = sub?.inner?;
        match which {
            Popup::View(view) => Some(TabAction::ViewMenu { view, id }),
            Popup::Switcher => Some(TabAction::SwitcherMenu { id }),
            _ => None,
        }
    });
    let on_opener = ui.ctx().pointer_latest_pos().is_some_and(|p| opener.contains(p));
    let pressed_outside = opened != ui.ctx().cumulative_frame_nr()
        && ui.ctx().input(|i| i.pointer.any_pressed())
        && !resp.response.contains_pointer()
        && !in_submenu
        && !on_opener;
    if selected.is_some() || pressed_outside || ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        pen.popup = None;
    }
    if let Some((view, row)) = next_menu {
        pen.popup = Some((Popup::View(view), row.right_top(), row, ui.ctx().cumulative_frame_nr()));
        ui.ctx().request_repaint();
    }
    ui.ctx().data_mut(|d| {
        if let Some(row) = flyout.open.filter(|_| pen.popup.is_some_and(|(popup, _, _, from)| popup == which && from == opened)) {
            d.insert_temp(flyout_id, (which, opened, row));
        } else {
            d.remove::<(Popup, u64, usize)>(flyout_id);
        }
    });
    pen.actions.extend(selected);
}

/// Discard view-index popup and drag state when the host changes the shown design.
/// Document-tab menus and the document list keep their independent state.
pub fn clear_view_popup(ctx: &egui::Context, strip: egui::Id) {
    ctx.data_mut(|data| {
        data.remove::<PopupState>(strip.with("view_popup"));
        data.remove::<ViewDrag>(strip.with("view_drag"));
    });
}

/// The selected design strip can receive a view from another window.
#[derive(Debug)]
pub struct ViewDropZone {
    pub document: usize,
    pub rect: Rect,
    pub view_centres: Vec<(usize, f32)>,
}

/// The laid-out opening, shared by the strip and the drag preview.
pub fn view_drag_gap(ctx: &egui::Context, strip: egui::Id, index: usize) -> Option<Rect> {
    ctx.data(|data| data.get_temp::<(usize, Rect)>(strip.with("view_drag_gap")))
        .filter(|(current, rect)| *current == index && rect.is_positive())
        .map(|(_, rect)| rect)
}

/// Leave room for an insertion before the given local index. Clear it when the pointer leaves.
pub fn set_view_drag_slot(ctx: &egui::Context, strip: egui::Id, index: Option<usize>, width: f32) {
    let id = strip.with("view_drag_slot");
    let target = index.filter(|_| width.is_finite() && width > 0.0).map(|index| (index, width));
    let changed = ctx.data_mut(|data| {
        if data.get_temp::<(usize, f32)>(id) == target {
            return false;
        }
        if let Some(slot) = target {
            data.insert_temp(id, slot);
        } else {
            data.remove::<(usize, f32)>(id);
        }
        true
    });
    if changed {
        ctx.request_repaint();
    }
}

/// The strip owns this drag until the user releases the pointer or cancels it.
pub fn view_drag_active(ctx: &egui::Context, strip: egui::Id) -> bool {
    ctx.data(|d| d.get_temp::<ViewDrag>(strip.with("view_drag")).is_some_and(|drag| !drag.cancelled))
        && ctx.input(|i| i.pointer.primary_down() || i.pointer.primary_released())
}

/// Draws the document tabs in `r` from `x0` to `x1`: click to select, × to close, drag to
/// reorder, a right-click for the tab's menu, » for every tab when they don't all fit, + for a
/// new one. The selected design with several views expands to show them (see [`DocTab::views`]).
pub fn tabs(ui: &mut egui::Ui, strip: &mut TabStrip, r: Rect, x0: f32, x1: f32) -> Vec<TabAction> {
    tabs_with_view_drop_zones(ui, strip, r, x0, x1).0
}

/// Draw the strip and report the current drop geometry in window coordinates.
pub fn tabs_with_view_drop_zones(ui: &mut egui::Ui, strip: &mut TabStrip, r: Rect, x0: f32, x1: f32) -> (Vec<TabAction>, Vec<ViewDropZone>) {
    let t = Tokens::get();
    let h = strip.handles;
    let n = strip.tabs.len();
    let mut view_drop_zones = Vec::new();
    let mut actions = Vec::new();
    // Each tab fits its name (up to MAX_FIT) while all of them fit; otherwise they share the room.
    const MAX_FIT: f32 = 200.0;
    // The selected tab grows with a name being typed into it.
    let typed = strip.rename.as_ref().map(|r| r.text.clone());
    let view_typed = strip.view_rename.as_ref().map(|(k, r)| (*k, r.text.clone()));
    let new_tab_width = if strip.pop_out { 0.0 } else { 34.0 };
    let min_views = if strip.switcher_always && !strip.views_as_tabs { 1 } else { 2 };
    let expands =
        |tab: &DocTab| strip.shown_selected == Some(strip.selected) && (tab.views.len() >= min_views || strip.pop_out || strip.view_rename.is_some());
    let geom = strip.tabs.get(strip.selected).copied().filter(expands).map(|tab| {
        let room = (x1 - x0 - new_tab_width - MIN_TAB * n.saturating_sub(1) as f32).max(0.0);
        measure(ui, strip, &tab, typed.as_deref().unwrap_or(tab.name), view_typed.as_ref().map(|(k, text)| (*k, text.as_str())), room)
    });
    let selected = strip.selected;
    let kind_of = |i: usize| geom.as_ref().filter(|_| i == selected).map_or(SelectedLayout::Plain, |g| g.kind);
    let (first, shown, w) = tab_layout(n, strip.selected, x1 - x0 - geom.as_ref().map_or(0.0, |g| (g.width - MIN_TAB).max(0.0)));
    let natural: Vec<f32> = strip
        .tabs
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            if let Some(g) = geom.as_ref().filter(|_| i == strip.selected) {
                return g.width;
            }
            let shown_name = match &typed {
                Some(text) if i == strip.selected => text.clone(),
                _ => tab.name.to_string(),
            };
            let label = format!("{shown_name}{}", if tab.dirty { " •" } else { "" });
            let text = text_width(ui, &label, crate::type_face::semibold(t.font_button));
            let badge = if tab.security.is_some() { 18.0 } else { 0.0 };
            let plus = if small_plus(strip, i) { SMALL_PLUS } else { 0.0 };
            let count = if tab.views.len() > 1 { 7.0 + badge_width(ui, tab.views.len()) } else { 0.0 };
            (text + 40.0 + badge + plus + count).clamp(72.0, MAX_FIT)
        })
        .collect();
    let fit_all = shown == n && natural.iter().sum::<f32>() <= x1 - x0 - new_tab_width;
    let menu_id = strip.id.with("menu");
    let mut menu: Option<(usize, Pos2)> = ui.ctx().data(|d| d.get_temp(menu_id));
    let (popup_id, vdrag_id) = (strip.id.with("view_popup"), strip.id.with("view_drag"));
    let (held_down, released, escape) = ui.input(|i| (i.pointer.primary_down(), i.pointer.primary_released(), i.key_pressed(egui::Key::Escape)));
    let mut pen = Pen {
        rename: strip.view_rename.take(),
        actions: Vec::new(),
        popup: ui.ctx().data(|d| d.get_temp::<PopupState>(popup_id)).flatten(),
        drag: ui.ctx().data(|d| d.get_temp::<ViewDrag>(vdrag_id)).filter(|_| held_down || released).map(|mut drag| {
            drag.cancelled |= escape;
            drag
        }),
        close_design: false,
        below: r.bottom(),
        view_centres: Vec::new(),
    };
    // The shown tabs' widths, in strip order.
    let widths: Vec<f32> = (first..first + shown)
        .map(|i| match geom.as_ref() {
            Some(g) if i == strip.selected => g.width,
            _ if fit_all => natural.get(i).copied().unwrap_or(w),
            _ => w,
        })
        .collect();
    let width = |i: usize| i.checked_sub(first).and_then(|k| widths.get(k)).copied().unwrap_or(w);
    let strip_end = x0 + widths.iter().sum::<f32>();
    // A tab being dragged to a new place follows the pointer and the others make room for it;
    // Escape puts it back.
    let drag_id = strip.id.with("drag");
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
    let (mut pick, mut shut, mut torn) = (None, None, None);
    for i in order {
        let Some(tab) = strip.tabs.get(i).copied() else {
            continue;
        };
        let w = width(i);
        let left = match held {
            Some((from, left)) if from == i => left,
            _ => i.checked_sub(first).and_then(|k| lefts.get(k)).copied().unwrap_or(x0),
        };
        let kind = kind_of(i);
        let plain = kind == SelectedLayout::Plain;
        let height = match kind {
            SelectedLayout::Plain => r.height() - 8.0,
            SelectedLayout::Pill => DESIGN_PILL_HEIGHT.min(r.height() - 4.0),
            SelectedLayout::Switcher => 30.0_f32.min(r.height() - 4.0),
        };
        let tr = Rect::from_center_size(pos2(left + (w - 2.0) / 2.0, r.center().y), vec2(w - 2.0, height));
        // An expanded tab's name block is what selects and drags the design: its views are theirs.
        let (dot, name_w) = geom.as_ref().filter(|_| !plain && i == selected).map_or((0.0, 0.0), |g| (g.dot, g.name_w));
        let hit = if plain { tr } else { Rect::from_min_size(tr.min, vec2((13.0 + dot + name_w + 10.0).min(tr.width()), tr.height())) };
        let active = strip.shown_selected == Some(i);
        let resp = ui.interact(hit, ui.scope_id().with(("doc_tab", i)), Sense::click_and_drag());
        solidcraft_ui_registry::publish_target(&format!("{}{i}", h.tab), hit);
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
            // Released well below the bar the tab leaves it; over the bar (not dropped somewhere
            // down the window) the tab lands there.
            if let Some((from, to)) = to {
                if pointer.is_some_and(|p| is_tear_off(p.y, r.bottom(), strip.tear_off_below)) {
                    torn = Some(from);
                } else if pointer.is_some_and(|p| p.y < r.bottom() + r.height()) {
                    dropped = Some((from, to));
                }
            }
            drag = None;
        }
        // An expanded tab draws its frame and views before its name goes over it.
        match (kind, geom.as_ref()) {
            (SelectedLayout::Pill, Some(g)) => draw_pill(ui, strip, &tab, tr, g, &mut pen),
            (SelectedLayout::Switcher, Some(g)) => draw_switcher(ui, strip, &tab, tr, g, &mut pen),
            _ => {}
        }
        view_drop_zones.push(ViewDropZone { document: i, rect: tr, view_centres: std::mem::take(&mut pen.view_centres) });
        let lit = resp.hovered() || held.is_some_and(|(from, _)| from == i);
        if plain {
            tab_shape(ui.painter(), &t, tr, active, lit || resp.has_focus());
        }
        let ink = if active || lit { t.app_bar_text } else { t.app_bar_text.gamma_multiply(0.7) };
        let label = format!("{}{}", tab.name, if tab.dirty && plain { " •" } else { "" });
        let font = if active { crate::type_face::semibold(t.font_button) } else { FontId::proportional(t.font_button) };
        let g = ui.painter().layout_no_wrap(label, font, ink);
        let text_size = g.size();
        let badge = if tab.security.is_some() { 18.0 } else { 0.0 };
        let plus = if small_plus(strip, i) { SMALL_PLUS } else { 0.0 };
        let count = if tab.views.len() > 1 && plain { 7.0 + badge_width(ui, tab.views.len()) } else { 0.0 };
        let (text_x, text_w) =
            if plain { (tr.left() + 10.0, (tr.width() - 30.0 - badge - plus - count).max(10.0)) } else { (tr.left() + 13.0 + dot, name_w - badge) };
        let tip = tab_tip(tab.name, tab.place, text_size.x > text_w);
        let clip = Rect::from_min_size(tr.min, vec2(text_x - tr.left() + text_w, tr.height()));
        match strip.rename.as_mut().filter(|_| active) {
            Some(rename) => {
                let field = Rect::from_min_max(pos2(tr.left() + 6.0, tr.top() + 3.0), pos2(text_x + text_w, tr.bottom() - 3.0));
                actions.extend(rename_field(ui, field, rename, &h));
            }
            None => {
                ui.painter().with_clip_rect(clip).galley(pos2(text_x, tr.center().y - text_size.y / 2.0), g, ink);
            }
        }
        if dot > 0.0 && tab.dirty {
            ui.painter().circle_filled(pos2(tr.left() + 13.0 + 3.5, tr.center().y), 3.5, t.warning);
        }
        let shield_x = if plain { tr.right() - 31.0 - plus } else { text_x + text_w + 11.0 };
        if let Some(badge) = tab.security {
            let br = Rect::from_center_size(pos2(shield_x, tr.center().y), vec2(14.0, 14.0));
            crate::icons::paint_ink(ui.painter(), br, "shield", badge.ink(&t));
        }
        if !strip.pop_out && resp.secondary_clicked() {
            menu = Some((i, ui.ctx().pointer_interact_pos().unwrap_or(tr.left_bottom())));
        }
        // A middle click closes the tab, as in a web browser.
        let mut closing = !strip.pop_out && (resp.middle_clicked() || (!plain && pen.close_design));
        if plain {
            if tab.views.len() > 1 {
                let at = pos2(text_x + text_size.x.min(text_w) + 7.0, tr.center().y);
                count_badge(ui, at, tab.views.len(), false);
            }
            if plus > 0.0 {
                let pr = Rect::from_center_size(pos2(tr.right() - 35.0, tr.center().y), vec2(20.0, 20.0));
                plus_button(ui, strip, pr, 12.0, &mut pen);
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
            // Named apart from a dialog's Close: egui names an unnamed widget after its tooltip.
            xresp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, show_close, strip.close_name));
            closing |= xresp.middle_clicked() || xresp.backing_on_hover_text(strip.close_tip).clicked();
        }
        if closing {
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
    ui.ctx().data_mut(|d| match pen.drag {
        Some(v) => {
            d.insert_temp(vdrag_id, v);
        }
        None => d.remove::<ViewDrag>(vdrag_id),
    });
    // In the order they happened: a later choice this frame (the list's, the menu's) wins.
    actions.extend(shut.map(TabAction::Close));
    actions.extend(pick.map(TabAction::Select));
    actions.append(&mut pen.actions);
    let mut x = strip_end;
    // More tabs than fit: a chevron lists them all.
    if !strip.pop_out && shown < n {
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
            .show(ui.ctx(), |ui| crate::menu::menu_frame(ui, |ui| menu_rows(ui, strip.menu, i == strip.selected, n > 1, "", h.menu_row, None)));
        if let Some(id) = &resp.inner {
            actions.push(TabAction::Menu { tab: i, id: id.clone() });
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
    // The menus of the selected design's views.
    view_popups(ui, strip, &mut pen);
    ui.ctx().data_mut(|d| d.insert_temp(popup_id, pen.popup));
    actions.append(&mut pen.actions);
    // A new tab.
    if strip.pop_out {
        return (actions, view_drop_zones);
    }
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
    actions.extend(torn.map(TabAction::TearOff));
    (actions, view_drop_zones)
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
                let row = crate::Item::action(if item.id.starts_with("appbar:overflow:") { &item.id } else { &item.label }, &item.label, &item.icon)
                    .key(&item.key)
                    .on_if(item.enabled);
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
    let close = chosen.is_some() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) || (!just_opened && resp.response.clicked_elsewhere());
    FileMenuOut { chosen, close }
}

/// A hidden control, with its original action and enabled state.
#[derive(Clone, Debug, PartialEq)]
pub struct OverflowItem {
    pub id: String,
    pub label: String,
    pub icon: String,
    pub enabled: bool,
    pub action: BarAction,
}

fn overflow_items(data: &AppBar, ids: &[String], anchor: f32) -> Vec<OverflowItem> {
    ids.iter()
        .map(|id| {
            let action = || BarAction::App { id: id.clone() };
            let mut row = OverflowItem { id: id.clone(), label: String::new(), icon: String::new(), enabled: true, action: action() };
            let mut button = None;
            match id.as_str() {
                "home" => {
                    button = Some(data.home);
                    row.label = data.home.tip.to_string();
                    row.action = BarAction::Home;
                }
                "settings" => {
                    button = Some(data.settings);
                    row.label = solidcraft_ui_text::tr("ui.preferences").to_string();
                    row.action = BarAction::Button { id: data.settings.id.to_string() };
                }
                "help" => {
                    button = Some(data.help);
                    row.label = data.help.tip.to_string();
                    row.action = BarAction::Help { x: anchor };
                }
                "report" => {
                    row.label = data.report.label.to_string();
                    row.icon = "bug_report".to_string();
                    row.action = BarAction::Report;
                }
                "version" => {
                    row.label = data.version.map_or("", |v| v.text).to_string();
                    row.icon = "help".to_string();
                    row.action = BarAction::Version;
                }
                _ => {
                    if let Some(i) = id.strip_prefix("action:").and_then(|i| i.parse::<usize>().ok()).and_then(|i| data.actions.get(i)) {
                        button = Some(*i);
                        let key = match i.id {
                            "save" => "ui.save",
                            "undo" => "ui.undo",
                            "redo" => "ui.redo",
                            _ => "",
                        };
                        row.label = if key.is_empty() { i.tip.to_string() } else { solidcraft_ui_text::tr(key).to_string() };
                        row.action = BarAction::Button { id: i.id.to_string() };
                    }
                    if let Some(p) = id.strip_prefix("pill:").and_then(|i| i.parse::<usize>().ok()).and_then(|i| data.pills.get(i)) {
                        row.label = p.name.to_string();
                        row.icon = p.icon.to_string();
                        row.action = BarAction::Pill { id: p.id.to_string() };
                    }
                }
            }
            if let Some(b) = button {
                row.icon = b.icon.to_string();
                row.enabled = b.enabled;
            }
            row
        })
        .collect()
}

/// Call after show: draw the More menu while its button is open, using the File menu surface.
/// Returns the original chosen action and closes after a choice, Escape or an outside click.
/// App-owned rows must have their label, icon and availability filled by the host.
pub fn overflow_menu(ctx: &egui::Context, rows: &[OverflowItem], button: Option<Rect>) -> Option<BarAction> {
    let id = egui::Id::unique("sc_appbar_overflow_open");
    let Some(button) = button else {
        ctx.data_mut(|d| d.insert_temp(id, false));
        return None;
    };
    if !ctx.data(|d| d.get_temp::<bool>(id).unwrap_or(false)) {
        return None;
    }
    let menu_rows: Vec<FileRow> = rows
        .iter()
        .map(|row| {
            FileRow::Item(FileItem {
                id: format!("appbar:overflow:{}", row.id),
                label: row.label.clone(),
                key: String::new(),
                icon: row.icon.clone(),
                enabled: row.enabled,
            })
        })
        .collect();
    let just_opened = ctx.data(|d| d.get_temp::<u64>(id.with("opened"))) == Some(ctx.cumulative_pass_nr());
    let out = file_menu(ctx, &menu_rows, button.left_bottom(), id.with("area"), just_opened);
    if out.close {
        ctx.data_mut(|d| d.insert_temp(id, false));
    }
    out.chosen.and_then(|chosen| rows.iter().find(|r| format!("appbar:overflow:{}", r.id) == chosen && r.enabled).map(|r| r.action.clone()))
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
