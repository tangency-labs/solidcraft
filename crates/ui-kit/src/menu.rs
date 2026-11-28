//! Menus share one popup surface and one row painter. Command dispatch stays with callers.
use crate::{BackingTooltipExt as _, icons};
use crate::{Container, Icon, Surface};
use egui::{Color32, FontId, Rect, Response, Sense, Stroke, TextureId, Ui, pos2, vec2};
use serde::Serialize;
use serde_json::Value;
use solidcraft_theme::Tokens;
use std::sync::Arc;

#[path = "menu_popup.rs"]
mod popup;
pub use popup::{MenuAnchor, MenuOutput, place};

/// The picture on an [`Item`]: a name in the kit's icon set, or a host's own picture. An item
/// owns its data, so it cannot hold a borrowed [`Icon`]. It reads as the kit name, which is empty
/// for a host's picture, so reports and the surfaces that only draw kit icons stay as they were.
#[derive(Clone, Default)]
pub struct ItemIcon {
    name: String,
    host: Option<HostIcon>,
}

#[derive(Clone)]
enum HostIcon {
    Texture(TextureId),
    Paint(Arc<dyn Fn(&egui::Painter, Rect, f32) + Send + Sync>),
}

impl ItemIcon {
    /// A texture the host made.
    pub fn texture(texture: TextureId) -> Self {
        Self { name: String::new(), host: Some(HostIcon::Texture(texture)) }
    }
    /// The host's own painter, called with the icon's rect and opacity.
    pub fn paint(paint: impl Fn(&egui::Painter, Rect, f32) + Send + Sync + 'static) -> Self {
        Self { name: String::new(), host: Some(HostIcon::Paint(Arc::new(paint))) }
    }
    pub fn paint_named(name: impl Into<String>, paint: impl Fn(&egui::Painter, Rect, f32) + Send + Sync + 'static) -> Self {
        Self { name: name.into(), host: Some(HostIcon::Paint(Arc::new(paint))) }
    }
    pub(crate) fn as_icon(&self) -> Icon<'_> {
        match &self.host {
            Some(HostIcon::Texture(texture)) => Icon::Texture(*texture),
            Some(HostIcon::Paint(paint)) => Icon::Paint(&**paint),
            None => Icon::Kit(&self.name),
        }
    }
}

impl std::ops::Deref for ItemIcon {
    type Target = str;
    fn deref(&self) -> &str {
        &self.name
    }
}

impl From<&str> for ItemIcon {
    fn from(name: &str) -> Self {
        Self { name: name.into(), host: None }
    }
}

impl From<String> for ItemIcon {
    fn from(name: String) -> Self {
        Self { name, host: None }
    }
}

impl PartialEq for ItemIcon {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && match (&self.host, &other.host) {
                (None, None) => true,
                (Some(HostIcon::Texture(a)), Some(HostIcon::Texture(b))) => a == b,
                (Some(HostIcon::Paint(a)), Some(HostIcon::Paint(b))) => Arc::ptr_eq(a, b),
                _ => false,
            }
    }
}

impl PartialEq<&str> for ItemIcon {
    fn eq(&self, other: &&str) -> bool {
        self.name == *other
    }
}

impl std::fmt::Display for ItemIcon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.name.fmt(f)
    }
}

impl std::fmt::Debug for ItemIcon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.name.fmt(f)
    }
}

impl Serialize for ItemIcon {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.name)
    }
}

/// A live numeric menu option, expressed in the units shown to the user.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MenuNumber {
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub speed: f64,
    pub suffix: String,
}

/// One menu entry: a command id (run with `params` when given, else started like a toolbar
/// click) or a `ui.*` action. `-` is a separator, `#…` a section heading.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Item {
    pub id: String,
    pub label: String,
    pub shortcut: String,
    pub enabled: bool,
    pub icon: ItemIcon,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Item>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swatch: Option<[u8; 3]>,
    /// Why the entry is off: shown on hover, and in the control channel's report.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tip: Option<String>,
    /// Explicit checkbox or radio state; None is an ordinary action.
    pub checked: Option<bool>,
    pub radio: bool,
    pub selected: bool,
    pub danger: bool,
    pub accent: Option<String>,
    /// Additional complete control-channel handles, independent of translated labels.
    pub aliases: Vec<String>,
    /// A secondary action such as pinning a command, emitted like any other Item.
    pub secondary: Option<Box<Item>>,
    /// Picking a toggle can keep its menu open.
    pub keep_open: bool,
    /// A row with children can also run its own action on click or Enter.
    pub submenu_action: bool,
    pub indent: usize,
    pub number: Option<MenuNumber>,
    pub context: Vec<Item>,
    pub draggable: bool,
    pub lazy_children: bool,
    #[serde(skip)]
    pub widget_id: Option<egui::Id>,
}

impl Default for Item {
    fn default() -> Self {
        Self {
            id: String::new(),
            label: String::new(),
            shortcut: String::new(),
            enabled: true,
            icon: ItemIcon::default(),
            params: None,
            children: Vec::new(),
            swatch: None,
            tip: None,
            checked: None,
            radio: false,
            selected: false,
            danger: false,
            accent: None,
            aliases: Vec::new(),
            secondary: None,
            keep_open: false,
            submenu_action: false,
            indent: 0,
            number: None,
            context: Vec::new(),
            draggable: false,
            lazy_children: false,
            widget_id: None,
        }
    }
}

/// Share of a menu row's width the note on the right may take before it is elided, so that a
/// long file name cannot cover the label beside it (#17302).
const KEY_ROOM: f32 = 0.5;

/// Lays out the note on the right of a menu row, elided to `room` points.
fn laid_out_key(painter: &egui::Painter, text: &str, room: f32, t: &Tokens) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(solidcraft_ui_text::tr_label(text).to_string(), FontId::monospace(t.font_small), t.text_dim, room);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('\u{2026}');
    painter.layout_job(job)
}

impl Item {
    /// A view action (`ui.*`).
    pub fn action(id: &str, label: &str, icon: &str) -> Item {
        Item {
            id: id.into(),
            label: label.into(),
            shortcut: String::new(),
            enabled: true,
            icon: icon.into(),
            params: None,
            children: Vec::new(),
            swatch: None,
            tip: None,
            ..Default::default()
        }
    }
    pub fn sep() -> Item {
        Item {
            id: "-".into(),
            label: String::new(),
            shortcut: String::new(),
            enabled: false,
            icon: ItemIcon::default(),
            params: None,
            children: Vec::new(),
            swatch: None,
            tip: None,
            ..Default::default()
        }
    }
    pub fn heading(label: &str) -> Item {
        Item {
            id: format!("#{label}"),
            label: label.into(),
            shortcut: String::new(),
            enabled: false,
            icon: ItemIcon::default(),
            params: None,
            children: Vec::new(),
            swatch: None,
            tip: None,
            ..Default::default()
        }
    }
    pub fn is_sep(&self) -> bool {
        self.id == "-"
    }
    pub fn is_heading(&self) -> bool {
        self.id.starts_with('#')
    }
    pub fn with(mut self, params: Value) -> Item {
        self.params = Some(params);
        self
    }
    pub fn on_if(self, enabled: bool) -> Item {
        self.on(enabled)
    }
    pub fn on(mut self, enabled: bool) -> Item {
        self.enabled &= enabled;
        self
    }
    /// Off, saying why, when there is a reason.
    pub fn off_because(mut self, why: Option<String>) -> Item {
        if let Some(why) = why {
            self.enabled = false;
            self.tip = Some(why);
        }
        self
    }
    pub fn key(mut self, k: &str) -> Item {
        self.shortcut = k.into();
        self
    }
    pub fn submenu(id: &str, label: &str, icon: &str, children: Vec<Item>) -> Self {
        let enabled = children.iter().any(|item| item.enabled && !item.is_sep() && !item.is_heading());
        Self { children, enabled, ..Self::action(id, label, icon) }
    }
    pub fn lazy_submenu(id: &str, label: &str, icon: &str) -> Self {
        Self { lazy_children: true, ..Self::action(id, label, icon) }
    }
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }
    pub fn alias(mut self, handle: &str) -> Self {
        self.aliases.push(handle.into());
        self
    }
    pub fn keep_open(mut self, keep_open: bool) -> Self {
        self.keep_open = keep_open;
        self
    }
    pub fn secondary(mut self, action: Item) -> Self {
        self.secondary = Some(Box::new(action));
        self
    }
}

/// Width of list menus.
pub const LIST_W: f32 = 250.0;

pub struct Menu;

/// Seconds the pointer rests on another row before an open flyout closes: a path to the flyout
/// that crosses a neighbouring row keeps it open.
pub const SUBMENU_CLOSE_DELAY: f64 = 0.3;

impl Menu {
    /// Below this contrast with `dialog_body`, a menu's rule all but vanishes on the menu surface
    /// (#15806), as when a theme tunes `borders.tertiary` for `panel` rather than `dialog_body`.
    pub const SEPARATOR_MIN_CONTRAST: f64 = 1.18;

    /// Stroke for menu rules, preserving visible or explicitly hidden rules while rescuing
    /// hairline rules whose contrast against `dialog_body` would otherwise vanish.
    pub fn separator_stroke(t: &Tokens) -> Stroke {
        let stroke = t.tertiary_border(t.dialog_rule);
        let visible = |c: Color32| {
            solidcraft_theme::contrast_ratio(solidcraft_theme::composite(c, t.dialog_body), t.dialog_body) >= Self::SEPARATOR_MIN_CONTRAST
        };
        if stroke.width <= 0.0 || stroke.color == Color32::TRANSPARENT || visible(stroke.color) {
            return stroke;
        }
        let fallback = [t.dialog_rule, t.dialog_border, t.border, t.form_dim].into_iter().find(|&c| visible(c));
        Stroke::new(stroke.width, fallback.unwrap_or(stroke.color))
    }

    fn pointer_moved(ctx: &egui::Context) -> bool {
        ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO)
    }

    /// A row opens its flyout when the pointer moves onto it, so a menu that opens under a still
    /// pointer (or is driven with the keys) opens none.
    pub fn flyout_hover(row: &Response) -> bool {
        row.hovered() && Self::pointer_moved(&row.ctx)
    }

    /// The pointer rests on another row of the parent menu since `leave`: true once the open
    /// flyout should close after [`SUBMENU_CLOSE_DELAY`] (#6657).
    pub fn flyout_leaving(ctx: &egui::Context, leave: &mut Option<f64>) -> bool {
        let now = ctx.input(|i| i.time);
        let left = SUBMENU_CLOSE_DELAY - (now - *leave.get_or_insert(now));
        if left <= 0.0 || !left.is_finite() {
            *leave = None;
            return true;
        }
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(left));
        false
    }

    /// Where a flyout of `size` sits beside its `row`: on the right with its first row level with
    /// `row`, on the left only when the right has no room (#8267), and kept on screen. A flyout of
    /// a flyout that opened to the left (`leftward`) opens to the left too while there is room, so
    /// it never covers the menu.
    pub fn flyout_at(screen: Rect, row: Rect, size: egui::Vec2, leftward: bool) -> egui::Pos2 {
        let margin = Self::frame().total_margin();
        let gap = margin.right + 2.0;
        let right = row.right() + gap;
        let left = row.left() - gap - size.x;
        let x = if right + size.x <= screen.right() && !(leftward && left >= screen.left()) { right } else { left.max(screen.left()) };
        let y = (row.top() - margin.top).min(screen.bottom() - size.y).max(screen.top());
        pos2(x, y)
    }

    pub fn frame() -> egui::Frame {
        super::Container::menu_frame()
    }

    pub(super) fn show_frame<R>(ui: &mut Ui, frame: egui::Frame, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
        let bounds = ui.max_rect();
        Container::new(ui.ctx(), Surface::Popup).show_frame(ui, frame, bounds, |ui| {
            ui.visuals_mut().widgets.noninteractive.bg_stroke = Self::separator_stroke(&Tokens::get());
            add(ui)
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn show<R>(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
        Self::show_frame(ui, Self::frame(), |ui| {
            ui.set_width((width - Tokens::get().space_6 * 2.0 - 2.0).max(0.0));
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), add).inner
        })
        .inner
    }

    pub fn separator(ui: &mut Ui) {
        let t = Tokens::get();
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.space_10 + 1.0), Sense::hover());
        ui.painter().hline(r.x_range(), r.center().y, Self::separator_stroke(&t));
    }

    pub fn heading(ui: &mut Ui, label: &str) {
        let t = Tokens::get();
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.space_24), Sense::hover());
        let mut job = egui::text::LayoutJob::simple_singleline(
            solidcraft_ui_text::tr_label(label).to_uppercase(),
            crate::type_face::semibold(t.font_small),
            t.form_dim,
        );
        for section in &mut job.sections {
            section.format.extra_letter_spacing = t.font_small * t.font_section_tracking;
        }
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(pos2(r.left() + t.space_8, r.center().y - galley.size().y / 2.0), galley, t.form_dim);
    }

    /// Bounded keyboard navigation skips disabled entries and structural rows.
    pub fn navigate(ctx: &egui::Context, enabled: &[bool], selected: &mut Option<usize>) -> bool {
        let (up, down, enter, home, end) = ctx.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                input.consume_key(egui::Modifiers::NONE, egui::Key::Home),
                input.consume_key(egui::Modifiers::NONE, egui::Key::End),
            )
        });
        let usable: Vec<_> = enabled.iter().enumerate().filter_map(|(k, on)| on.then_some(k)).collect();
        if home || end {
            *selected = if home { usable.first() } else { usable.last() }.copied();
        } else if up || down {
            let at = selected.and_then(|r| usable.iter().position(|u| *u == r));
            let len = usable.len().max(1);
            let next = match (at, down) {
                (None, true) => 0,
                (None, false) => len - 1,
                (Some(k), true) => (k + 1) % len,
                (Some(k), false) => (k + len - 1) % len,
            };
            *selected = usable.get(next).copied();
        }
        enter && selected.is_some_and(|k| enabled.get(k).copied().unwrap_or(false))
    }
}

/// Shared row states. Swatches are document colour data; chrome always uses Tokens.
pub struct MenuItem<'a> {
    pub show_label: bool,
    pub suppress_pointer_hot: bool,
    pub publish_handle: bool,
    pub command_label: bool,
    pub id: &'a str,
    pub label: &'a str,
    pub icon: Icon<'a>,
    pub shortcut: &'a str,
    pub enabled: bool,
    pub highlighted: bool,
    pub checked: Option<bool>,
    pub swatch: Option<Color32>,
    pub danger: bool,
    pub submenu: bool,
    pub radio: bool,
    pub accent: Option<&'a str>,
    pub indent: usize,
}

impl<'a> MenuItem<'a> {
    pub fn new(id: &'a str, label: &'a str, icon: impl Into<Icon<'a>>) -> Self {
        Self {
            show_label: true,
            suppress_pointer_hot: false,
            publish_handle: true,
            command_label: true,
            id,
            label,
            icon: icon.into(),
            shortcut: "",
            enabled: true,
            highlighted: false,
            checked: None,
            swatch: None,
            danger: false,
            submenu: false,
            radio: false,
            accent: None,
            indent: 0,
        }
    }

    /// The row for a menu entry, `lit` when it is chosen with the keys or its flyout is open. A
    /// `current` shortcut marks the checked entry of a choice.
    pub fn from_item(item: &'a Item, lit: bool) -> Self {
        Self {
            show_label: true,
            suppress_pointer_hot: false,
            publish_handle: true,
            command_label: false,
            id: &item.id,
            label: &item.label,
            icon: item.icon.as_icon(),
            shortcut: if item.shortcut == "current" { "" } else { &item.shortcut },
            enabled: item.enabled,
            highlighted: lit || item.selected,
            danger: item.danger || item.icon == "delete",
            checked: item.checked.or_else(|| (item.shortcut == "current").then_some(true)),
            swatch: item.swatch.map(|c| Color32::from_rgb(c[0], c[1], c[2])),
            submenu: item.lazy_children || !item.children.is_empty(),
            radio: item.radio,
            accent: item.accent.as_deref(),
            indent: item.indent,
        }
    }

    pub(crate) fn trailing_check_width(icon: Icon<'_>, checkable: bool) -> f32 {
        let t = Tokens::get();
        if !icon.is_blank() && checkable { t.space_16 + t.space_8 } else { 0.0 }
    }

    fn translated_label(&self) -> &str {
        if self.command_label { solidcraft_ui_text::tr_command(self.id, self.label) } else { solidcraft_ui_text::tr_label(self.label) }
    }

    pub fn show(&self, ui: &mut Ui) -> Response {
        let t = Tokens::get();
        // Areas first measure intrinsic content. Filling their provisional available
        // width would make a flipped submenu cover its parent and consume the opening click.
        let width = if ui.is_sizing_pass() {
            let label = ui.painter().layout_no_wrap(self.translated_label().to_string(), FontId::proportional(t.font_label), t.form_ink);
            let shortcut = ui.painter().layout_no_wrap(self.shortcut.to_string(), FontId::monospace(t.font_small), t.form_dim);
            let trailing = if self.submenu { t.space_16 } else { 0.0 };
            let check = Self::trailing_check_width(self.icon, self.checked.is_some());
            (t.control_height + t.space_2 + label.size().x + shortcut.size().x + t.space_24 + trailing + check).min(ui.available_width())
        } else {
            ui.available_width()
        };
        let (r, response) =
            ui.allocate_exact_size(vec2(width, t.control_height + t.space_2), if self.enabled { Sense::click() } else { Sense::hover() });
        self.paint(ui, r, &response, 0.0);
        response
    }

    /// Ribbon rows retain their own drag/focus IDs and reserve a trailing pin target.
    pub fn paint(&self, ui: &Ui, r: Rect, response: &Response, trailing: f32) {
        let t = Tokens::get();
        let enabled = self.enabled && ui.is_enabled();
        let hot = enabled && (self.highlighted || (!self.suppress_pointer_hot && (response.hovered() || response.has_focus())));
        let ink = if self.danger {
            t.form_error
        } else if hot {
            t.form_primary
        } else {
            t.form_ink
        };
        let dim = if self.danger {
            t.form_error
        } else if hot {
            t.form_primary
        } else {
            t.form_dim
        };
        let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(r));
        let mut painter = painter;
        if !enabled {
            painter.multiply_opacity(0.4);
        }
        if hot {
            painter.rect_filled(r, t.control_radius, t.accent_soft);
        }
        let indent = t.space_16 * self.indent.min(16) as f32;
        let icon = Rect::from_center_size(pos2(r.left() + indent + t.control_height / 2.0, r.center().y), vec2(t.space_16, t.space_16));
        if let Some(on) = self.checked.filter(|_| self.icon.is_blank()) {
            if self.radio {
                painter.circle_stroke(icon.center(), t.space_4, Stroke::new(1.0, dim));
                if on {
                    painter.circle_filled(icon.center(), t.space_2, t.form_primary);
                }
            } else if on {
                super::checks::paint_check_glyph(&painter, icon, false, t.form_primary);
            }
        } else if let Some(color) = self.swatch {
            painter.rect(icon, t.radius_sm, color, t.control_border_stroke(t.control_border), egui::StrokeKind::Inside);
        } else if !self.icon.is_blank() {
            match self.icon {
                Icon::Kit(name @ ("window_here" | "window_other")) => icons::paint_ink(&painter, icon, name, t.text_dim),
                Icon::Kit(name) if self.danger => icons::paint_danger(&painter, icon, name, 1.0),
                Icon::Kit(name) if self.accent.is_some() => icons::paint_accented(&painter, icon, name, self.accent, 1.0),
                Icon::Kit(name) if hot => icons::paint_ink(&painter, icon, name, dim),
                plain => plain.paint_plain(&painter, icon, 1.0),
            }
        }
        let label = self.translated_label();
        let mut right = r.right() - t.space_8 - trailing;
        if self.submenu {
            let c = pos2(right - t.space_4, r.center().y);
            painter.add(egui::Shape::line(
                vec![c + vec2(-t.space_2, -t.space_4), c + vec2(t.space_2, 0.0), c + vec2(-t.space_2, t.space_4)],
                Stroke::new(1.0, dim),
            ));
            right -= t.space_16;
        }
        let check_width = Self::trailing_check_width(self.icon, self.checked.is_some());
        if check_width > 0.0 {
            if self.checked == Some(true) {
                let check = Rect::from_center_size(pos2(right - t.space_8, r.center().y), icon.size());
                if self.radio {
                    painter.circle_filled(check.center(), t.space_3, t.form_primary);
                } else {
                    super::checks::paint_check_glyph(&painter, check, false, t.form_primary);
                }
            }
            right -= check_width;
        }
        if !self.shortcut.is_empty() {
            // The note on the right is a file name for Insert Design's list, and it used to be
            // laid out with no width at all: a long name grew leftwards over the row's label
            // (#17302). Cap it to a share of the row and elide what does not fit.
            let room = (r.width() * KEY_ROOM).max(0.0);
            let whole = solidcraft_ui_text::tr_label(self.shortcut).to_string();
            let untruncated = painter.layout_no_wrap(whole.clone(), FontId::monospace(t.font_small), dim);
            let fits = untruncated.size().x <= room + 0.5;
            let key = if fits { untruncated } else { laid_out_key(&painter, self.shortcut, room, &t) };
            painter.galley(pos2(right - key.size().x, r.center().y - key.size().y / 2.0), key.clone(), dim);
            right -= key.size().x + t.space_8;
            // Cutting the note short can leave two rows that look the same: design names are free
            // text, and two file names can differ only past the ellipsis. A row that had to cut its
            // note says the whole name on hover, so the choice is not blind. `Item::tip` is not
            // used for this: it is the reason a row is off, and a row can be off and elided at once.
            if !fits && enabled {
                response.clone().backing_on_hover_text(whole);
            }
        }
        let left = r.left() + indent + t.control_height + t.space_2;
        let mut job = egui::text::LayoutJob::simple_singleline(label.to_string(), FontId::proportional(t.font_label), ink);
        job.wrap.max_width = (right - left).max(0.0);
        job.wrap.max_rows = 1;
        let galley = painter.layout_job(job);
        if self.show_label {
            painter.galley(pos2(left, r.center().y - galley.size().y / 2.0), galley, ink);
        }
        response.widget_info(|| match self.checked {
            Some(checked) => {
                egui::WidgetInfo::selected(if self.radio { egui::Role::RadioButton } else { egui::Role::CheckBox }, enabled, checked, label)
            }
            None => egui::WidgetInfo::labeled(egui::Role::Button, enabled, label),
        });
        if self.publish_handle {
            solidcraft_ui_registry::publish_target(&format!("menu:{}", self.id), r);
        }
        if enabled && response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
}

/// A list menu's popup surface, [`LIST_W`] wide, around `add`.
#[cfg(any(test, feature = "test-support"))]
pub fn menu_frame<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    Menu::show(ui, LIST_W, add)
}

/// Points per second a scroll bar of [`scroll_rows`] moves the rows while the pointer rests on it.
const SCROLL_BAR_SPEED: f32 = 480.0;

/// What [`scroll_rows`] remembers between frames under its id: the rows' measured height and
/// how far they are scrolled.
#[derive(Clone, Copy, Default)]
struct ScrollRows {
    content: f32,
    offset: f32,
}

/// `add`'s rows in at most `room` points of height. Rows that fit are drawn as they are. Taller
/// rows (`estimate` high until they have been measured once) scroll: a full-width bar with an
/// up chevron above them and one with a down chevron below them scroll while the pointer rests
/// on them and by a page when clicked, and the wheel scrolls too. A bar is dimmed at its end.
/// `id` keeps the scroll position, so a menu opened afresh should use a new one.
pub fn scroll_rows<R>(ui: &mut Ui, id: egui::Id, room: f32, estimate: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    scroll_rows_reveal(ui, id, room, estimate, None, add)
}

fn scroll_rows_reveal<R>(ui: &mut Ui, id: egui::Id, room: f32, estimate: f32, reveal: Option<(f32, f32)>, add: impl FnOnce(&mut Ui) -> R) -> R {
    let t = Tokens::get();
    let mut state = ui.ctx().data(|d| d.get_temp::<ScrollRows>(id)).unwrap_or(ScrollRows { content: estimate, offset: 0.0 });
    let room = if room.is_finite() { room.max(0.0) } else { 0.0 };
    if !(state.content.is_finite() && state.content > room + 0.5) {
        // Measure the rows as they are, so a list that grows past the room starts scrolling.
        let rows = ui.scope(add);
        state.content = rows.response.rect.height();
        ui.ctx().data_mut(|d| d.insert_temp(id, state));
        return rows.inner;
    }
    // Preserve a complete action row before spending the remaining room on bars.
    let row = t.control_height + t.space_2;
    let bar = t.control_height.min(((room - row) / 2.0).max(0.0));
    let view = (room - 2.0 * bar).max(0.0);
    let end = (state.content - view).max(0.0);
    let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.1);
    let mut offset = state.offset.clamp(0.0, end);
    if let Some((top, bottom)) = reveal {
        if top < offset {
            offset = top;
        } else if bottom > offset + view {
            offset = bottom - view;
        }
    }
    let up = scroll_bar(ui, id.with("up"), true, offset > 0.5, bar);
    if up.hovered() {
        offset -= SCROLL_BAR_SPEED * dt;
    }
    if up.clicked() {
        offset -= view * 0.8;
    }
    let offset = offset.clamp(0.0, end);
    let out = egui::ScrollArea::vertical()
        .id_salt(id.with("rows"))
        .max_height(view)
        .min_scrolled_height(view)
        .vertical_scroll_offset(offset)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, add);
    let mut offset = out.state.clamped_offset().y;
    let down = scroll_bar(ui, id.with("down"), false, offset < end - 0.5, bar);
    if down.hovered() {
        offset += SCROLL_BAR_SPEED * dt;
    }
    if down.clicked() {
        offset += view * 0.8;
    }
    if up.hovered() || down.hovered() {
        ui.ctx().request_repaint();
    }
    let content = out.content_size.y;
    let content = if content.is_finite() && content > 0.0 { content } else { state.content };
    ui.ctx().data_mut(|d| d.insert_temp(id, ScrollRows { content, offset: offset.clamp(0.0, (content - view).max(0.0)) }));
    out.inner
}

/// One of [`scroll_rows`]' full-width bars: a chevron pointing `up` or down, inert and dimmed
/// when the rows cannot scroll that way.
fn scroll_bar(ui: &mut Ui, id: egui::Id, up: bool, active: bool, height: f32) -> Response {
    let t = Tokens::get();
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let response = ui.interact(r, id, if active { Sense::click() } else { Sense::hover() });
    if height <= 0.0 {
        return response;
    }
    let hot = active && response.hovered();
    if hot {
        ui.painter().rect_filled(r, t.control_radius, t.accent_soft);
    }
    let ink = if hot { t.form_primary } else { t.form_dim };
    let ink = if active { ink } else { ink.gamma_multiply(0.4) };
    let c = r.center();
    let (w, h) = (t.space_8, if up { -t.space_4 } else { t.space_4 });
    ui.painter().add(egui::Shape::line(vec![c + vec2(-w, -h / 2.0), c + vec2(0.0, h / 2.0), c + vec2(w, -h / 2.0)], Stroke::new(1.5, ink)));
    let label = if up { "Scroll up" } else { "Scroll down" };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, active, label));
    solidcraft_ui_registry::publish_target(&format!("menu:scroll_{}", if up { "up" } else { "down" }), r);
    if active && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// A menu row that acts as a button.
pub fn button(ui: &mut Ui, label: &str, icon: &str) -> Response {
    button_enabled(ui, label, icon, true)
}

/// [`button`], greyed out and inert when `enabled` is false.
pub fn button_enabled(ui: &mut Ui, label: &str, icon: &str, enabled: bool) -> Response {
    let mut item = MenuItem::new(label, label, icon);
    item.enabled = enabled;
    item.show(ui)
}

/// One entry of a list menu: a separator, a heading or a row. Returns the entry when its row was
/// clicked while enabled.
pub fn list_row(ui: &mut Ui, item: &Item) -> Option<Item> {
    list_row_lit(ui, item, false)
}

/// [`list_row`] with the row lit (chosen with the keys, or its flyout open).
pub fn list_row_lit(ui: &mut Ui, item: &Item, lit: bool) -> Option<Item> {
    if item.is_sep() {
        Menu::separator(ui);
        return None;
    }
    if item.is_heading() {
        Menu::heading(ui, &item.label);
        return None;
    }
    let row = MenuItem::from_item(item, lit);
    (row.show(ui).clicked() && item.enabled).then(|| item.clone())
}

/// Rename keeps the stable editor identity and selects the whole initial name.
pub fn rename_menu(ui: &mut Ui, value: &mut String, focused: &mut bool) -> Option<bool> {
    let t = Tokens::get();
    let mut done = None;
    let bounds = ui.max_rect();
    let frame = Container::menu_frame();
    Container::new(ui.ctx(), Surface::Popup).show_frame(ui, frame, bounds, |ui| {
        ui.set_width(t.form_card_width);
        super::scope(ui, |ui| {
            Menu::heading(ui, "Rename");
            let id = egui::Id::unique("sc_rename_text");
            let response = ui.add(super::fields::text_input(egui::TextEdit::singleline(value).id(id)).desired_width(ui.available_width()));
            t.input_bevel(ui, &response);
            solidcraft_ui_registry::publish_target("rename:name", response.rect);
            if !*focused {
                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
                    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(value.chars().count()),
                    )));
                    state.store(ui.ctx(), id);
                }
                response.request_focus();
                *focused = true;
            }
            // Leaving the box with Tab keeps the new name as Enter does; only Escape cancels (#3313).
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Tab))) {
                done = Some(true);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                done = Some(false);
            }
            match super::footer(ui, "OK", "Cancel", !value.trim().is_empty(), super::Footer::default()) {
                super::Action::Ok => done = Some(true),
                super::Action::Cancel => done = Some(false),
                _ => {}
            }
        })
    });
    done
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{key, pointer};

    #[test]
    fn checkable_menu_rows_keep_their_icon_in_both_states() {
        const HOST: TextureId = TextureId::User(7);
        let ctx = crate::test_support::context();
        let item = Item { icon: ItemIcon::texture(HOST), ..Item::action("id", "Label", "") };
        for checked in [false, true] {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let mut row = MenuItem::new(&item.id, &item.label, item.icon.as_icon());
                row.checked = Some(checked);
                row.show(ui);
            });
            output.textures_delta.clear();
            assert!(output.shapes.iter().any(|clipped| matches!(&clipped.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == HOST)));
            assert_eq!(output.shapes.iter().any(|clipped| matches!(&clipped.shape, egui::Shape::Path(_))), checked);
        }
    }

    #[test]
    fn a_menu_row_draws_a_hosts_texture_and_an_item_still_reads_as_its_kit_name() {
        const HOST: TextureId = TextureId::User(7);
        let item = Item { icon: ItemIcon::texture(HOST), ..Item::action("id", "Label", "") };
        let mut output = crate::test_support::context().run_ui(egui::RawInput::default(), |ui| {
            MenuItem::new(&item.id, &item.label, item.icon.as_icon()).show(ui);
        });
        output.textures_delta.clear();
        assert!(output.shapes.iter().any(|clipped| matches!(&clipped.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == HOST)));

        assert_eq!(serde_json::to_value(&item.icon).unwrap(), serde_json::json!(""), "a host's picture has no name to report");
        let named = ItemIcon::from("delete");
        assert!(named == "delete" && matches!(named.as_icon(), Icon::Kit("delete")));
        assert_eq!(serde_json::to_value(&named).unwrap(), serde_json::json!("delete"));
    }

    #[test]
    fn a_long_note_on_the_right_of_a_row_is_elided_and_leaves_the_label_its_share() {
        const ROOM: f32 = 120.0;
        let ctx = crate::test_support::context();
        let mut medidas = None;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let t = Tokens::get();
            let painter = ui.painter().clone();
            medidas =
                Some((laid_out_key(&painter, "Topcraft_Messschieber_Unterteil_final_v3.sc", ROOM, &t), laid_out_key(&painter, "plate.sc", ROOM, &t)));
        });
        output.textures_delta.clear();
        let (key, short) = medidas.expect("the row laid the note out");
        assert!(key.elided, "a name wider than its room must be elided");
        assert!(key.size().x <= ROOM + 0.5, "elided note is {} wide, room is {ROOM}", key.size().x);
        assert!(!short.elided && short.size().x < ROOM, "a short note keeps its whole text");
    }

    /// A note that did not fit is cut short, so the row says the whole name on hover; a note that
    /// fits is drawn whole and adds no hover. Two designs whose file names differ past the ellipsis
    /// cannot be told apart by the row alone, which is what the hover is for.
    #[test]
    fn a_cut_short_note_says_its_whole_name_on_hover() {
        let long = "Topcraft_Messschieber_Unterteil_final_v3.sc";
        for (note, hovered) in [(long, true), ("plate.sc", false)] {
            let ctx = crate::test_support::context();
            ctx.global_style_mut(|style| style.interaction.tooltip_delay = 0.0);
            let row = Rect::from_min_size(egui::Pos2::ZERO, vec2(240.0, 24.0));
            let at = row.center();
            let mut texts = Vec::new();
            for frame in 0..4 {
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(260.0, 40.0))),
                    events: if frame == 0 { vec![egui::Event::PointerMoved(at)] } else { Vec::new() },
                    time: Some(f64::from(frame)),
                    ..Default::default()
                };
                let mut output = ctx.run_ui(input, |ui| {
                    ui.scope_builder(egui::UiBuilder::new().max_rect(row), |ui| {
                        let mut item = MenuItem::new("id", "Design", "");
                        item.shortcut = note;
                        item.show(ui);
                    });
                });
                output.textures_delta.clear();
                let tips: Vec<Rect> = ctx.memory(|m| {
                    m.areas()
                        .visible_layer_ids()
                        .into_iter()
                        .filter(|layer| layer.order == egui::Order::Tooltip)
                        .filter_map(|layer| m.area_rect(layer.id))
                        .collect()
                });
                texts = crate::test_support::text_shapes(&output)
                    .filter(|text| tips.iter().any(|tip| tip.contains(text.pos)))
                    .map(|text| text.galley.job.text.to_owned())
                    .collect();
            }
            let want: Vec<String> = if hovered { vec![note.to_owned()] } else { Vec::new() };
            assert_eq!(texts, want, "the hover of a row noted {note}");
        }
    }

    #[test]
    fn menu_navigation_skips_disabled_rows_and_wraps() {
        let ctx = crate::test_support::context();
        let mut selected = None;
        let mut accepted = false;
        let mut frame = |events| {
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                accepted = Menu::navigate(ui.ctx(), &[true, false, true], &mut selected);
            });
            output.textures_delta.clear();
            (selected, accepted)
        };
        assert_eq!(frame(key(egui::Key::ArrowDown)), (Some(0), false));
        assert_eq!(frame(key(egui::Key::ArrowDown)), (Some(2), false));
        assert_eq!(frame(key(egui::Key::ArrowDown)), (Some(0), false));
        assert_eq!(frame(key(egui::Key::ArrowUp)), (Some(2), false));
        assert_eq!(frame(key(egui::Key::Home)), (Some(0), false));
        assert_eq!(frame(key(egui::Key::End)), (Some(2), false));
        assert_eq!(frame(key(egui::Key::Enter)), (Some(2), true));
    }

    #[test]
    fn radial_list_placement_handles_narrow_windows_and_oversized_menus() {
        for screen in [Rect::from_min_size(pos2(0.0, 0.0), vec2(180.0, 720.0)), Rect::from_min_size(pos2(30.0, 20.0), vec2(800.0, 300.0))] {
            for list_height in [40.0, 500.0, 2000.0] {
                let center = Menu::marking_center(screen.center(), screen);
                let at = Menu::marking_list_place(center, list_height, screen);
                assert!(at.is_finite());
                assert!(screen.contains(at), "even a scrolling list starts inside its window");
            }
        }
    }

    #[test]
    fn shared_menu_adapter_does_not_reopen_after_its_owner_disappears() {
        let ctx = crate::test_support::context();
        let owner = egui::Id::unique("shared-menu-hidden-owner");
        let items = [Item::action("open", "Open", "")];
        let frame = |visible, open| {
            let mut result = MenuOutput::default();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                if visible {
                    let response = ui.interact(Rect::from_min_size(pos2(10.0, 10.0), vec2(80.0, 30.0)), owner, Sense::click());
                    if open {
                        Menu::open_button(&response);
                    }
                    result = Menu::button_items_output(&response, &items);
                }
            });
            output.textures_delta.clear();
            result
        };
        assert!(frame(true, true).open);
        assert!(frame(true, false).open, "a continuously rendered owner keeps its popup");
        for _ in 0..4 {
            frame(false, false);
        }
        let restored = frame(true, false);
        assert!(!restored.open && restored.rows.is_empty(), "returning to the owner requires a fresh opening gesture");
        assert!(frame(true, true).open, "an explicit programmatic opening still works");
    }

    #[test]
    fn inline_menu_before_an_editor_respects_text_focus_and_recovers_row_keys() {
        let ctx = crate::test_support::context();
        let id = egui::Id::unique("inline-editor-actions");
        let editor_id = id.with("comment");
        let items = [Item::action("delete", "Delete", "delete")];
        let mut comment = String::from("Comment");
        let mut frame = |events, focus_editor, standalone| {
            let mut result = MenuOutput::default();
            let mut remaining = Vec::new();
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                result = if standalone {
                    Menu::popup(ui.ctx(), id, MenuAnchor::Point(pos2(250.0, 10.0)), &items, false)
                } else {
                    Menu::items(ui, id, &items, false)
                };
                remaining = [egui::Key::Enter, egui::Key::ArrowLeft, egui::Key::ArrowRight]
                    .into_iter()
                    .filter(|key| ui.input(|input| input.key_pressed(*key)))
                    .collect();
                let editor = ui.add(egui::TextEdit::singleline(&mut comment).id(editor_id));
                if focus_editor {
                    editor.request_focus();
                }
            });
            output.textures_delta.clear();
            (result, remaining)
        };
        for pressed in [egui::Key::Enter, egui::Key::ArrowLeft, egui::Key::ArrowRight] {
            frame(vec![], true, false);
            assert!(ctx.text_edit_focused());
            let (result, remaining) = frame(key(pressed), false, false);
            assert!(result.picked.is_none() && !result.close, "editing must not activate Delete or dismiss the form");
            assert!(remaining.contains(&pressed), "the later editor still receives its key");
        }
        let (menu, _) = frame(vec![], false, false);
        ctx.memory_mut(|memory| memory.request_focus(menu.rows[0].1.id));
        assert!(!ctx.text_edit_focused());
        assert_eq!(frame(key(egui::Key::Enter), false, false).0.picked.map(|item| item.id), Some("delete".into()));
        frame(vec![], true, false);
        assert!(ctx.text_edit_focused());
        assert_eq!(
            frame(key(egui::Key::Enter), false, true).0.picked.map(|item| item.id),
            Some("delete".into()),
            "standalone context menus own their keys over a focused editor"
        );
    }

    #[test]
    fn inline_menu_opening_enter_does_not_pick_its_first_action() {
        let ctx = crate::test_support::context();
        let id = egui::Id::unique("inline-menu-opening-enter");
        let items = [Item::action("delete", "Delete", "delete")];
        let frame = |fresh| {
            let mut result = MenuOutput::default();
            let mut output = ctx.run_ui(egui::RawInput { events: key(egui::Key::Enter), ..Default::default() }, |ui| {
                result = Menu::items(ui, id, &items, fresh);
            });
            output.textures_delta.clear();
            result
        };
        let opened = frame(true);
        assert!(opened.open && !opened.close && opened.picked.is_none());
        assert_eq!(frame(false).picked.map(|item| item.id), Some("delete".into()));
        let reopened = frame(true);
        assert!(reopened.open && !reopened.close && reopened.picked.is_none(), "immediate reopening also protects the opener key");
    }

    #[test]
    fn shared_menu_rejects_changed_branch_actions_and_bounds_popups() {
        let ctx = crate::test_support::context();
        let id = egui::Id::unique("menu-changing-branch");
        let old = [Item::submenu("old", "Old", "", vec![Item::action("delete-old", "Delete old", "")])];
        let new = [Item::submenu("new", "New", "", vec![Item::action("delete-new", "Delete new", "")])];
        let frame = |items: &[Item], events| {
            let mut result = MenuOutput::default();
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                result = Menu::popup(ui.ctx(), id, MenuAnchor::Point(pos2(10.0, 10.0)), items, false);
            });
            output.textures_delta.clear();
            result
        };
        frame(&old, vec![]);
        frame(&old, vec![]);
        frame(&old, key(egui::Key::ArrowRight));
        frame(&old, vec![]);
        frame(&old, vec![]);
        let changed = frame(&new, key(egui::Key::Enter));
        assert!(changed.picked.is_none() && changed.depth == 0, "Enter cannot activate an unseen replacement branch");
        Menu::reveal(&ctx, id, &new, "delete-new");
        assert_eq!(frame(&new, key(egui::Key::Enter)).picked.map(|item| item.id), Some("delete-new".into()));
        let bounds = Rect::from_min_size(pos2(100.0, 100.0), vec2(280.0, 220.0));
        let rows: Vec<_> = (0..40).map(|i| Item::action(&i.to_string(), "Long menu action", "")).collect();
        for _ in 0..3 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let popup = Menu::popup_with_bounds(ui.ctx(), id.with("bounded"), MenuAnchor::Point(pos2(120.0, 280.0)), &rows, false, bounds);
                assert!(bounds.contains_rect(popup.rect.unwrap()));
            });
            output.textures_delta.clear();
        }
    }

    #[test]
    fn preview_does_not_open_an_interactive_menu_and_disabled_openers_stay_closed() {
        let ctx = crate::test_support::context();
        let id = egui::Id::unique("menu-preview-isolation");
        let items = [Item::action("action", "Action", "")];
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            Menu::reveal(ui.ctx(), id, &items, "action");
            Menu::preview(ui.ctx(), id, MenuAnchor::Point(pos2(200.0, 30.0)), &items);
            assert!(!Menu::is_open(ui.ctx(), id) && !Menu::any_open(ui.ctx()));
            let disabled = ui.add_enabled(false, egui::Button::new("Disabled"));
            Menu::open_button(&disabled);
            assert!(!Menu::button_items_output(&disabled, &items).open);
            assert!(!Menu::context_active(&disabled));
        });
        output.textures_delta.clear();
    }

    #[test]
    fn focused_more_button_never_activates_its_primary_remove_action() {
        let ctx = crate::test_support::context();
        let id = egui::Id::unique("secondary-menu-focus");
        let items = [Item::action("remove", "Remove", "delete").secondary(Item::submenu(
            "more",
            "More actions",
            "more",
            vec![Item::action("inspect", "Inspect", "")],
        ))];
        let frame = |events| {
            let mut result = MenuOutput::default();
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                result = Menu::popup(ui.ctx(), id, MenuAnchor::Point(pos2(20.0, 20.0)), &items, false);
            });
            output.textures_delta.clear();
            result
        };
        frame(vec![]);
        let painted = frame(vec![]);
        let more = painted.rows.iter().find(|(item, _)| item.id == "more").unwrap().1.clone();
        ctx.memory_mut(|memory| memory.request_focus(more.id));
        let entered = frame(key(egui::Key::Enter));
        assert!(entered.picked.is_none() && entered.open, "Enter on More cannot remove the primary item");
        frame(vec![]);
        let expanded = frame(vec![]);
        assert!(expanded.rows.iter().any(|(item, _)| item.id == "inspect"));
        assert!(frame(key(egui::Key::Escape)).open, "Escape closes only the auxiliary menu");
        assert!(ctx.memory(|memory| memory.has_focus(more.id)));
        let reopened = frame(key(egui::Key::Enter));
        assert!(reopened.picked.is_none() && reopened.open);
        frame(vec![]);
        assert!(frame(vec![]).rows.iter().any(|(item, _)| item.id == "inspect"));
    }

    #[test]
    fn popup_scrolling_fits_a_host_shorter_than_three_controls() {
        let ctx = crate::test_support::context();
        let bounds = Rect::from_min_size(pos2(100.0, 100.0), vec2(280.0, 64.0));
        let rows: Vec<_> = (0..12).map(|index| Item::action(&index.to_string(), "Action", "")).collect();
        for (index, anchor) in [MenuAnchor::Point(bounds.left_bottom()), MenuAnchor::Rect(Rect::from_center_size(bounds.center(), vec2(100.0, 24.0)))]
            .into_iter()
            .enumerate()
        {
            let id = egui::Id::unique(("tiny-menu-host", index));
            Menu::set_cursor(&ctx, id, 0, Some(11));
            for pass in 0..3 {
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    let result = Menu::popup_with_bounds(ui.ctx(), id, anchor, &rows, false, bounds);
                    assert!(bounds.contains_rect(result.rect.unwrap()), "popup {:?} exceeds host {:?}", result.rect, bounds);
                    if pass == 2 {
                        let focused = &result.rows.iter().find(|(item, _)| item.id == "11").unwrap().1;
                        assert_eq!(focused.interact_rect, focused.rect, "the selected action remains fully interactable in a short host");
                    }
                });
                output.textures_delta.clear();
            }
        }
    }

    #[test]
    fn shared_menu_keyboard_activity_ignores_stationary_hover_and_tracks_visible_bounds() {
        let ctx = crate::test_support::context();
        let id = egui::Id::unique("menu-hot-row");
        let items = [Item::action("a", "A", ""), Item::action("b", "B", "")];
        let frame = |events| {
            let mut result = MenuOutput::default();
            let mut hot = Vec::new();
            let mut contains = false;
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                result = Menu::popup(ui.ctx(), id, MenuAnchor::Point(pos2(20.0, 20.0)), &items, false);
                hot = result.rows.iter().filter(|(_, response)| Menu::row_is_hot(response)).map(|(item, _)| item.id.clone()).collect();
                contains = Menu::contains_pointer(ui.ctx());
                if result.sizing {
                    assert!(!contains, "invisible measuring bounds cannot keep a parent form open");
                }
            });
            output.textures_delta.clear();
            (result, hot, contains)
        };
        frame(vec![egui::Event::PointerMoved(pos2(50.0, 40.0))]);
        let (painted, _, contains) = frame(vec![]);
        assert!(contains);
        let at = painted.rows[0].1.rect.center();
        frame(vec![egui::Event::PointerMoved(at)]);
        frame(key(egui::Key::ArrowDown));
        let (moved, hot, _) = frame(key(egui::Key::ArrowDown));
        assert_eq!(hot, vec!["b".to_owned()]);
        assert_eq!(moved.active_row, Some(moved.rows[1].1.id));
        frame(vec![egui::Event::PointerMoved(pos2(1000.0, 1000.0))]);
        assert!(!frame(vec![]).2, "only actual visible menu bounds contain the pointer");
    }

    #[test]
    fn shared_menu_pointer_membership_is_scoped_to_its_source_layer() {
        let ctx = crate::test_support::context();
        let items = [Item::action("pick", "Pick", "")];
        let mut output = ctx.run_ui(egui::RawInput { events: vec![egui::Event::PointerMoved(pos2(40.0, 10.0))], ..Default::default() }, |ui| {
            let source = ui.layer_id();
            Menu::items(ui, egui::Id::unique("embedded-membership"), &items, false);
            assert!(Menu::contains_pointer(ui.ctx()));
            assert!(Menu::contains_pointer_in(ui.ctx(), source));
            assert!(!Menu::contains_pointer_in(ui.ctx(), egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("unrelated-form"))));
        });
        output.textures_delta.clear();
    }

    #[test]
    fn menu_escape_cancels_an_activation_from_the_same_input_frame() {
        let ctx = crate::test_support::context();
        let id = egui::Id::unique("menu-simultaneous-cancel");
        let items = [Item::action("remove", "Remove", "delete")];
        let frame = |events| {
            let mut result = MenuOutput::default();
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                result = Menu::popup(ui.ctx(), id, MenuAnchor::Point(pos2(20.0, 20.0)), &items, false);
            });
            output.textures_delta.clear();
            result
        };
        frame(vec![]);
        frame(vec![]);
        Menu::set_cursor(&ctx, id, 0, Some(0));
        let events = key(egui::Key::Enter).into_iter().chain(key(egui::Key::Escape)).collect();
        let cancelled = frame(events);
        assert!(cancelled.close && !cancelled.open);
        assert!(cancelled.picked.is_none(), "the caller must not dispatch Remove after Escape cancelled its menu");
    }

    #[test]
    fn tall_rect_anchored_roots_scroll_on_one_side_without_covering_the_opener() {
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(640.0, 720.0));
        for (top, below) in [(80.0_f32, true), (620.0, false), (350.0, false)] {
            let ctx = crate::test_support::context();
            let id = egui::Id::unique(("side-bounded-root", top.to_bits()));
            let anchor = Rect::from_min_size(pos2(50.0, top), vec2(100.0, 24.0));
            let items: Vec<_> = (0..50).map(|index| Item::action(&index.to_string(), &format!("Action {index}"), "")).collect();
            let frame = |events| {
                let mut result = MenuOutput::default();
                let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| {
                    result = Menu::popup(ui.ctx(), id, MenuAnchor::Rect(anchor), &items, false);
                });
                output.textures_delta.clear();
                result
            };
            frame(vec![]);
            frame(vec![]);
            let opened = frame(vec![]);
            let bounds = opened.rect.unwrap();
            assert!(screen.contains_rect(bounds));
            assert!(!bounds.intersect(anchor).is_positive(), "scrolling root {bounds:?} must not cover opener {anchor:?}");
            if below {
                assert!(bounds.top() >= anchor.bottom());
            } else {
                assert!(bounds.bottom() <= anchor.top());
            }
            frame(key(egui::Key::End));
            let revealed = frame(vec![]);
            let last = &revealed.rows.last().unwrap().1;
            assert_eq!(last.interact_rect, last.rect, "the final action is reachable after scrolling");
            assert!(revealed.rect.unwrap().contains_rect(last.rect));
            assert_eq!(frame(key(egui::Key::Enter)).picked.map(|item| item.id), Some("49".into()));
        }
    }

    #[test]
    fn middle_point_popup_shows_all_rows_when_the_usable_window_fits_them() {
        let ctx = crate::test_support::context();
        let id = egui::Id::unique("point-popup-full-fit");
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(640.0, 1000.0));
        let mut items = vec![Item::heading("Select Other")];
        items.extend((0..20).map(|index| Item::action(&index.to_string(), &format!("Choice {index}"), "")));
        for _ in 0..3 {
            solidcraft_ui_registry::clear_handles();
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
                let popup = Menu::popup(ui.ctx(), id, MenuAnchor::Point(pos2(300.0, 500.0)), &items, false);
                let bounds = popup.rect.unwrap();
                let last = &popup.rows.last().unwrap().1;
                assert!(screen.contains_rect(bounds));
                assert!(bounds.contains_rect(last.rect), "all twenty rows fit without scrolling: {:?} in {bounds:?}", last.rect);
                assert_eq!(last.interact_rect, last.rect);
                assert!(solidcraft_ui_registry::rect_of("menu:scroll_down").is_none(), "placement and scroll height use the same window bottom");
            });
            output.textures_delta.clear();
        }
    }

    #[test]
    fn shared_menu_preserves_explicit_labels_and_scrolls_only_on_keyboard_requests() {
        let ctx = crate::test_support::context();
        let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(420.0, 260.0));
        let id = egui::Id::unique("shared-menu-scroll");
        let mut items = vec![Item::action("file.export_stl", "STL…", "")];
        items.extend((0..50).map(|index| Item::action(&format!("row.{index}"), &format!("Row {index}"), "")));
        items.push(Item::sep());
        items.push(Item::action("disabled", "Disabled", "").on_if(false));
        let frame = |events| {
            let mut result = MenuOutput::default();
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| {
                result = Menu::popup(ui.ctx(), id, MenuAnchor::Point(pos2(10.0, 10.0)), &items, false);
            });
            let labels: Vec<_> = crate::test_support::text_shapes(&output).map(|text| text.galley.job.text.clone()).collect();
            output.textures_delta.clear();
            (result, labels)
        };
        frame(vec![]);
        assert!(frame(vec![]).1.iter().any(|label| label == "STL…"));
        frame(key(egui::Key::End));
        assert_eq!(Menu::cursor(&ctx, id, 0), Some(50));
        let (visible, _) = frame(vec![]);
        let last = visible.rows.iter().find(|(item, _)| item.id == "row.49").unwrap().1.rect;
        assert!(visible.rect.unwrap().intersects(last), "End reveals the last enabled row");
        frame(key(egui::Key::ArrowUp));
        assert_eq!(Menu::cursor(&ctx, id, 0), Some(49));
        let (before_wheel, _) = frame(vec![]);
        let original = before_wheel.rows.iter().find(|(item, _)| item.id == "row.48").unwrap().1.rect.top();
        let (wheel, _) = frame(vec![
            egui::Event::PointerMoved(pos2(100.0, 120.0)),
            egui::Event::MouseWheel {
                source: egui::MouseWheelSource::Unknown,
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, 180.0),
                modifiers: egui::Modifiers::NONE,
                phase: egui::TouchPhase::Move,
            },
        ]);
        let before = wheel.rows.iter().find(|(item, _)| item.id == "row.48").unwrap().1.rect.top();
        let (still, _) = frame(vec![]);
        let after = still.rows.iter().find(|(item, _)| item.id == "row.48").unwrap().1.rect.top();
        assert!(after > original + 1.0, "the wheel must actually move the list toward its earlier rows");
        assert!(after >= before - 1.0, "idle cursor must not undo wheel scrolling");
    }

    #[test]
    fn shared_menu_can_open_in_background_and_closes_on_a_focus_loss_transition() {
        let ctx = crate::test_support::context();
        let id = egui::Id::unique("shared-menu-background");
        let items = [Item::action("open", "Open", "")];
        let frame = |events, fresh| {
            let mut result = MenuOutput::default();
            let mut output = ctx.run_ui(egui::RawInput { focused: false, events, ..Default::default() }, |ui| {
                result = Menu::popup(ui.ctx(), id, MenuAnchor::Point(pos2(20.0, 20.0)), &items, fresh);
            });
            output.textures_delta.clear();
            result
        };
        assert!(frame(vec![], true).open);
        assert!(frame(vec![], false).open, "automation may interact with a background window");
        assert!(frame(vec![egui::Event::WindowFocused(false)], false).close);
    }

    #[test]
    fn shared_menu_keeps_parent_handles_and_walks_nested_keyboard_rows() {
        let ctx = crate::test_support::context();
        let id = egui::Id::unique("nested-menu-identity");
        let inner =
            Item::submenu("appearance", "Appearance", "", vec![Item::action("appearance", "Appearance", ""), Item::action("swatch", "Blue", "")]);
        let items = [Item::submenu("component", "Component", "", vec![Item::action("rename", "Rename", ""), inner])];
        let frame = |events| {
            solidcraft_ui_registry::clear_handles();
            let mut result = MenuOutput::default();
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                result = Menu::popup(ui.ctx(), id, MenuAnchor::Point(pos2(20.0, 20.0)), &items, false);
            });
            output.textures_delta.clear();
            result
        };
        frame(vec![]);
        frame(vec![]);
        assert_eq!(frame(key(egui::Key::ArrowRight)).depth, 1);
        frame(vec![]);
        frame(vec![]);
        frame(key(egui::Key::ArrowDown));
        frame(vec![]);
        assert_eq!(Menu::cursor(&ctx, id, 1), Some(1));
        assert_eq!(frame(key(egui::Key::ArrowRight)).depth, 2);
        frame(vec![]);
        let output = frame(vec![]);
        let parent = output.rows.iter().find(|(item, _)| item.id == "appearance").unwrap().1.rect;
        assert_eq!(solidcraft_ui_registry::rect_of("menu:appearance"), Some(parent));
        assert!(!Menu::level_rect(&ctx, id, 1).unwrap().intersects(Menu::level_rect(&ctx, id, 2).unwrap()));
        frame(key(egui::Key::ArrowLeft));
        assert!(Menu::level_rect(&ctx, id, 2).is_none());
        assert_eq!(Menu::cursor(&ctx, id, 1), Some(1));
    }

    #[test]
    fn overlapping_flyout_requires_a_separate_child_click() {
        let ctx = crate::test_support::context();
        let id = egui::Id::unique("narrow-overlapping-flyout");
        let items = [Item::submenu("create", "Create", "", vec![Item::action("extrude", "Extrude", "")])];
        let frame = |events| {
            let mut result = MenuOutput::default();
            let mut output = ctx.run_ui(
                egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 400.0))), events, ..Default::default() },
                |ui| {
                    result = Menu::popup(ui.ctx(), id, MenuAnchor::Point(pos2(5.0, 10.0)), &items, false);
                },
            );
            output.textures_delta.clear();
            result
        };
        frame(vec![]);
        let at = frame(vec![]).rows[0].1.rect.center();
        frame(vec![egui::Event::PointerMoved(pos2(199.0, 399.0))]);
        frame(vec![egui::Event::PointerMoved(at)]);
        frame(vec![]);
        let expanded = frame(vec![]);
        assert!(
            expanded.rows.iter().any(|(item, response)| item.id == "extrude" && response.rect.contains(at)),
            "opener {at:?}, depth {}, rows {:?}",
            expanded.depth,
            expanded.rows.iter().map(|(item, response)| (&item.id, response.rect)).collect::<Vec<_>>()
        );
        let click = || {
            [true, false]
                .map(|pressed| egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                })
                .to_vec()
        };
        assert!(frame(click()).picked.is_none(), "the opening click cannot run the child under its opener");
        assert_eq!(frame(click()).picked.map(|item| item.id), Some("extrude".into()));
    }

    #[test]
    fn shared_menu_announces_checked_and_radio_state() {
        let ctx = crate::test_support::context();
        ctx.enable_accesskit();
        let mut radio = Item::action("radio", "Radio", "").checked(true);
        radio.radio = true;
        let items = [Item::action("toggle", "Toggle", "").checked(false), radio];
        for _ in 0..3 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                Menu::items(ui, egui::Id::unique("checked-menu"), &items, false);
            });
            let nodes = &output.platform_output.accesskit_update.as_ref().unwrap().nodes;
            assert!(nodes.iter().any(|(_, node)| node.label() == Some("Toggle")
                && node.role() == egui::accesskit::Role::CheckBox
                && node.toggled() == Some(egui::accesskit::Toggled::False)));
            assert!(nodes.iter().any(|(_, node)| node.label() == Some("Radio")
                && node.role() == egui::accesskit::Role::RadioButton
                && node.toggled() == Some(egui::accesskit::Toggled::True)));
            output.textures_delta.clear();
        }
    }

    #[test]
    fn shared_menu_flyouts_fit_small_screens_and_keyboard_returns_a_stable_action() {
        let ctx = crate::test_support::context();
        let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(320.0, 220.0));
        let id = egui::Id::unique("shared-menu-keyboard");
        let items = vec![
            Item::submenu(
                "style",
                "Style",
                "",
                vec![Item::action("style.a", "A", "").checked(true), Item::action("style.b", "B", "").checked(false)],
            ),
            Item::action("disabled", "Disabled", "").on_if(false),
            Item::action("close", "Close", ""),
        ];
        let frame = |events| {
            let mut result = MenuOutput::default();
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| {
                result = Menu::popup(ui.ctx(), id, MenuAnchor::Rect(Rect::from_min_size(pos2(276.0, 4.0), vec2(32.0, 26.0))), &items, false);
            });
            output.textures_delta.clear();
            result
        };
        frame(vec![]);
        assert!(screen.contains_rect(frame(vec![]).rect.unwrap()));
        Menu::set_cursor(&ctx, id, 0, Some(0));
        assert_eq!(frame(key(egui::Key::ArrowRight)).depth, 1);
        frame(key(egui::Key::ArrowDown));
        let picked = frame(key(egui::Key::Enter));
        assert_eq!(picked.picked.as_ref().map(|item| item.id.as_str()), Some("style.b"));
        assert!(picked.close && !picked.open);
        assert!(!ctx.input(|input| input.key_pressed(egui::Key::Enter)), "menu keys are consumed before camera input");
    }

    #[test]
    fn shared_menu_skips_disabled_rows_preserves_toggle_data_and_bounds_stale_paths() {
        let ctx = crate::test_support::context();
        let id = egui::Id::unique("shared-menu-toggle");
        let toggle = Item::action("toggle", "Translated label", "").checked(false).keep_open(true).with(serde_json::json!({"target":17}));
        let items = vec![Item::action("off", "Off", "").on_if(false), Item::sep(), toggle.clone()];
        let frame = |events| {
            let mut result = MenuOutput::default();
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                result = Menu::popup(ui.ctx(), id, MenuAnchor::Point(pos2(20.0, 20.0)), &items, false);
            });
            output.textures_delta.clear();
            result
        };
        Menu::set_cursor(&ctx, id, 0, Some(usize::MAX));
        frame(key(egui::Key::ArrowDown));
        assert_eq!(Menu::cursor(&ctx, id, 0), Some(2));
        let picked = frame(key(egui::Key::Enter));
        assert_eq!(picked.picked, Some(toggle));
        assert!(picked.open && !picked.close);
        Menu::reveal(&ctx, id, &items, "not-present");
        assert!(Menu::open_path(&ctx, id).is_empty());
        Menu::dismiss(&ctx, id);
        assert!(!Menu::is_open(&ctx, id));
    }

    #[test]
    fn shared_menu_state_is_scoped_to_its_window_and_stale_definitions_are_safe() {
        let ctx = crate::test_support::context();
        let id = egui::Id::unique("same-menu-id-in-two-windows");
        let other = egui::ViewportId(egui::Id::unique("other-menu-window"));
        let items = vec![Item::submenu("parent", "Parent", "", vec![Item::action("child", "Child", "")])];
        let input = egui::RawInput { viewport_id: egui::ViewportId::ROOT, ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| {
            Menu::popup(ui.ctx(), id, MenuAnchor::Point(pos2(20.0, 20.0)), &items, false);
            assert!(Menu::is_open(ui.ctx(), id));
            assert!(Menu::any_open(ui.ctx()));
        });
        output.textures_delta.clear();
        let mut input = egui::RawInput { viewport_id: other, ..Default::default() };
        input.viewports.insert(other, egui::ViewportInfo::default());
        let mut output = ctx.run_ui(input, |ui| {
            assert!(!Menu::is_open(ui.ctx(), id), "another window cannot inherit this menu's state");
            assert!(!Menu::any_open(ui.ctx()), "menus in another window cannot block its controls");
            Menu::reveal(ui.ctx(), id, &items, "child");
            assert_eq!(Menu::open_path(ui.ctx(), id), vec![0]);
            // The definition can change between frames, including removal of the open parent.
            Menu::popup(ui.ctx(), id, MenuAnchor::Point(pos2(f32::NAN, f32::INFINITY)), &[], false);
            assert!(Menu::open_path(ui.ctx(), id).is_empty());
            Menu::set_cursor(ui.ctx(), id, usize::MAX, Some(usize::MAX));
        });
        output.textures_delta.clear();
    }

    #[test]
    fn shared_menu_row_metadata_preserves_dragging_and_previews_cannot_drag() {
        let ctx = crate::test_support::context();
        let id = egui::Id::unique("shared-menu-drag");
        let mut item = Item::action("command", "Command", "");
        item.draggable = true;
        let items = vec![item];
        let frame = |events, preview| {
            let mut result = MenuOutput::default();
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                result = if preview {
                    Menu::preview(ui.ctx(), id, MenuAnchor::Point(pos2(20.0, 20.0)), &items)
                } else {
                    Menu::popup(ui.ctx(), id, MenuAnchor::Point(pos2(20.0, 20.0)), &items, false)
                };
            });
            output.textures_delta.clear();
            result
        };
        frame(vec![], false);
        let bounds = frame(vec![], false).rows[0].1.rect;
        let at = bounds.center();
        frame(
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE },
            ],
            false,
        );
        let drag = frame(vec![egui::Event::PointerMoved(at + vec2(40.0, 0.0))], false);
        assert!(drag.rows.iter().any(|(item, response)| item.id == "command" && response.drag_started()), "the host receives the real drag response");
        let preview = frame(vec![egui::Event::PointerMoved(at + vec2(60.0, 0.0))], true);
        assert!(preview.rows.iter().all(|(_, response)| !response.drag_started() && !response.enabled()));
        assert!(preview.picked.is_none());
    }

    #[test]
    fn shared_menu_lazy_branches_keep_their_identity_until_the_builder_fills_them() {
        let ctx = crate::test_support::context();
        let id = egui::Id::unique("shared-menu-lazy");
        let mut items = vec![Item::lazy_submenu("body:1", "Body 1", "")];
        let frame = |items: &[Item], events| {
            let mut result = MenuOutput::default();
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                result = Menu::popup(ui.ctx(), id, MenuAnchor::Point(pos2(20.0, 20.0)), items, false);
            });
            output.textures_delta.clear();
            result
        };
        Menu::set_cursor(&ctx, id, 0, Some(0));
        frame(&items, key(egui::Key::ArrowRight));
        assert!(Menu::branch_open(&ctx, id, "body:1"));
        items[0].children.push(Item::action("face:1", "Face 1", ""));
        Menu::reveal(&ctx, id, &items, "face:1");
        let result = frame(&items, key(egui::Key::Enter));
        assert_eq!(result.picked.as_ref().map(|item| item.id.as_str()), Some("face:1"));
    }

    #[test]
    fn flyouts_open_right_of_their_row_and_flip_left_only_without_room() {
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 600.0));
        let size = vec2(200.0, 100.0);
        let top = Menu::frame().total_margin().top;
        let row = Rect::from_min_max(pos2(100.0, 50.0), pos2(400.0, 80.0));
        let at = Menu::flyout_at(screen, row, size, false);
        assert!(at.x > row.right() && at.x + size.x <= screen.right(), "{at:?}");
        assert_eq!(at.y, row.top() - top);
        // No room on the right: the flyout sits left of the row, still clear of it.
        let edge = row.translate(vec2(500.0, 0.0));
        let at = Menu::flyout_at(screen, edge, size, false);
        assert!(at.x + size.x < edge.left() && at.x >= screen.left(), "{at:?}");
        // A row near the bottom keeps its flyout on screen.
        let low = row.translate(vec2(0.0, 500.0));
        assert_eq!(Menu::flyout_at(screen, low, size, false).y, screen.bottom() - size.y);
        // Leftward keeps to the left while it fits there, and goes right where it does not.
        let middle = Rect::from_min_max(pos2(300.0, 50.0), pos2(500.0, 80.0));
        let at = Menu::flyout_at(screen, middle, size, true);
        assert!(at.x + size.x < middle.left() && at.x >= screen.left(), "{at:?}");
        assert!(Menu::flyout_at(screen, row, size, true).x > row.right());
    }

    #[test]
    fn disabled_menu_rows_are_inert_and_live_rows_accept_clicks() {
        for enabled in [false, true] {
            let ctx = crate::test_support::context();
            let target = std::cell::Cell::new(Rect::NOTHING);
            let clicked = std::cell::Cell::new(false);
            let mut draw = |ui: &mut Ui| {
                Menu::show(ui, 268.0, |ui| {
                    let mut item = MenuItem::new("paste", "Paste", "paste");
                    item.enabled = enabled;
                    let response = item.show(ui);
                    target.set(response.rect);
                    clicked.set(clicked.get() || response.clicked());
                });
            };
            super::super::test_support::frame(&ctx, vec![], &mut draw);
            let at = target.get().center();
            super::super::test_support::frame(&ctx, pointer(at, true), &mut draw);
            super::super::test_support::frame(&ctx, pointer(at, false), &mut draw);
            assert_eq!(clicked.get(), enabled);
        }
    }

    #[test]
    fn separator_stroke_preserves_hidden_and_visible_rules_and_rescues_vanishing_ones() {
        for base in [Tokens::light(), Tokens::dark()] {
            let stroke = Menu::separator_stroke(&base);
            assert_eq!(stroke, base.tertiary_border(base.dialog_rule));
            assert!(solidcraft_theme::contrast_ratio(stroke.color, base.dialog_body) >= Menu::SEPARATOR_MIN_CONTRAST);
        }

        let mut hidden_zero = Tokens::dark();
        hidden_zero.borders.tertiary = Some(Stroke::new(0.0, Color32::WHITE));
        assert_eq!(Menu::separator_stroke(&hidden_zero).width, 0.0);

        let mut hidden_alpha = Tokens::dark();
        hidden_alpha.borders.tertiary = Some(Stroke::new(1.0, Color32::TRANSPARENT));
        assert_eq!(Menu::separator_stroke(&hidden_alpha).color.a(), 0);

        let mut blueprint_like = Tokens::dark();
        blueprint_like.dialog_body = Color32::from_rgb(0x11, 0x2d, 0x4b);
        blueprint_like.dialog_rule = Color32::from_rgb(0x25, 0x47, 0x64);
        blueprint_like.dialog_border = Color32::from_rgb(0x31, 0x57, 0x77);
        blueprint_like.borders.tertiary = Some(Stroke::new(1.5, Color32::from_rgb(0x10, 0x27, 0x48)));
        assert_eq!(Menu::separator_stroke(&blueprint_like), Stroke::new(1.5, blueprint_like.dialog_rule));

        let mut rule_vanishes = blueprint_like;
        rule_vanishes.dialog_rule = blueprint_like.dialog_body;
        assert_eq!(Menu::separator_stroke(&rule_vanishes), Stroke::new(1.5, rule_vanishes.dialog_border));

        let mut border_vanishes = rule_vanishes;
        border_vanishes.dialog_border = border_vanishes.dialog_body;
        assert_eq!(Menu::separator_stroke(&border_vanishes), Stroke::new(1.5, border_vanishes.border));

        let mut all_borders_vanish = border_vanishes;
        all_borders_vanish.border = all_borders_vanish.dialog_body;
        assert_eq!(Menu::separator_stroke(&all_borders_vanish), Stroke::new(1.5, all_borders_vanish.form_dim));

        let mut transparent_fallback = Tokens::light();
        transparent_fallback.borders.tertiary = Some(Stroke::new(1.0, transparent_fallback.dialog_body));
        transparent_fallback.dialog_rule = Color32::TRANSPARENT;
        assert_eq!(Menu::separator_stroke(&transparent_fallback), Stroke::new(1.0, transparent_fallback.dialog_border));

        for base in [Tokens::light(), Tokens::dark()] {
            let mut translucent = base;
            translucent.borders.tertiary = Some(Stroke::new(1.0, Color32::from_white_alpha(8)));
            assert_eq!(Menu::separator_stroke(&translucent), Stroke::new(1.0, base.dialog_rule));
        }
    }
}
