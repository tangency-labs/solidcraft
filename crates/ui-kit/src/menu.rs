//! Menus share one popup surface and one row painter. Command dispatch stays with callers.
use crate::{Container, Icon, Surface};
use crate::{icons, type_face::RichTextExt as _};
use egui::{Color32, FontId, Rect, Response, Sense, Stroke, TextureId, Ui, pos2, vec2};
use serde::Serialize;
use serde_json::Value;
use solidcraft_theme::Tokens;
use std::sync::Arc;

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

/// One menu entry: a command id (run with `params` when given, else started like a toolbar
/// click) or a `ui.*` action. `-` is a separator, `#…` a section heading.
#[derive(Clone, Debug, Serialize)]
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

    pub fn context<R>(response: &Response, add: impl FnOnce(&mut Ui) -> R) -> Option<egui::InnerResponse<R>> {
        Container::new(&response.ctx, Surface::Popup).context_menu(response).frame(Self::frame()).show(|ui| {
            ui.visuals_mut().widgets.noninteractive.bg_stroke = Self::separator_stroke(&Tokens::get());
            add(ui)
        })
    }

    pub fn button<R>(ui: &mut Ui, label: impl Into<egui::WidgetText>, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<Option<R>> {
        Self::button_with(
            ui,
            |ui, nested| {
                let button = egui::Button::new(label.into());
                ui.add(if nested { button.right_text(egui::menu::SubMenuButton::arrow_atom(None)) } else { button })
            },
            add,
        )
    }

    pub fn text<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<Option<R>> {
        Self::button_with(ui, |ui, _| super::text_button(ui, label, true), add)
    }

    /// A row whose popup opens beside it, on the left only when the right has no room (#8267).
    pub fn item_button<R>(ui: &mut Ui, item: &MenuItem<'_>, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<Option<R>> {
        Self::button_at(ui, |ui, _| item.show(ui), add, true)
    }

    pub fn button_with<R>(
        ui: &mut Ui,
        button: impl FnOnce(&mut Ui, bool) -> Response,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> egui::InnerResponse<Option<R>> {
        Self::button_at(ui, button, add, false)
    }

    fn button_at<R>(
        ui: &mut Ui,
        button: impl FnOnce(&mut Ui, bool) -> Response,
        add: impl FnOnce(&mut Ui) -> R,
        beside: bool,
    ) -> egui::InnerResponse<Option<R>> {
        let previous = ui.style().clone();
        let frame = Self::frame();
        let style = ui.style_mut();
        style.spacing.menu_margin = frame.inner_margin;
        style.visuals.menu_corner_radius = frame.corner_radius;
        style.visuals.popup_shadow = frame.shadow;
        style.visuals.window_fill = frame.fill;
        style.visuals.window_stroke = frame.stroke;
        let nested = egui::menu::is_in_menu(ui);
        let response = button(ui, nested);
        let popup_id = if nested { egui::menu::SubMenu::id_from_widget_id(response.id) } else { egui::Popup::default_response_id(&response) };
        let was_open = ui.ctx().read_response(popup_id).is_some();
        let gate_id = popup_id.with("opening_click");
        let first_click = !was_open || ui.ctx().data(|d| d.get_temp::<bool>(gate_id)).unwrap_or(true);
        let (opening_press, released) = ui.input(|i| {
            (
                first_click
                    && (i.pointer.any_down() || i.pointer.any_released())
                    && i.pointer.press_origin().or_else(|| i.pointer.interact_pos()).is_some_and(|p| response.rect.contains(p)),
                i.pointer.any_released(),
            )
        });
        let content = |ui: &mut Ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.visuals_mut().widgets.noninteractive.bg_stroke = Self::separator_stroke(&Tokens::get());
            // The opening gesture belongs to the parent even when a narrow popup overlaps it.
            ui.ctx().data_mut(|d| d.insert_temp(gate_id, first_click && !released));
            let result = ui.add_enabled_ui(!opening_press, add).inner;
            Container::finish_content(ui, frame);
            result
        };
        let popup = if nested {
            let submenu = Container::new(ui.ctx(), Surface::Popup).submenu();
            let submenu = if opening_press {
                submenu.config(egui::menu::MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside))
            } else {
                submenu
            };
            let slot = super::container::BackingSlot::reserve(ui.ctx(), egui::LayerId::new(egui::Order::Foreground, popup_id));
            submenu.show(ui, &response, |ui| match slot {
                Some(slot) => slot.body(ui, content),
                None => content(ui),
            })
        } else if beside {
            let open = Self::flyout_open(ui, &response, popup_id);
            let popup = Container::new(ui.ctx(), Surface::Popup).menu(&response).open(open).frame(frame);
            // Last pass's size; the first pass assumes the flyout fits on the right.
            let size = popup.get_expected_size().unwrap_or(egui::Vec2::ZERO);
            let at = Self::flyout_at(ui.ctx().content_rect(), response.interact_rect, size, false);
            popup.anchor(at).align(egui::RectAlign::BOTTOM_START).align_alternatives(&[]).gap(0.0).show(content)
        } else {
            Container::new(ui.ctx(), Surface::Popup).menu(&response).frame(frame).show(content)
        };
        let response = egui::InnerResponse { response, inner: popup.map(|r| r.inner) };
        ui.set_style(previous);
        response
    }

    /// Hovering a row opens its flyout, like the context menu's. The open flyout stays while the
    /// pointer is on its row or inside it, and closes after resting elsewhere in the parent menu.
    fn flyout_open(ui: &Ui, row: &Response, popup_id: egui::Id) -> bool {
        // (open flyout, when the pointer left it, the pass that last showed it, the pointer has
        // moved since the parent menu opened)
        type State = (Option<egui::Id>, Option<f64>, u64, bool);
        let key = ui.scope_id().with("sc_flyout");
        let ctx = ui.ctx();
        let pass = ctx.cumulative_pass_nr();
        let (mut open, mut leave, seen, mut moved) = ctx.data(|d| d.get_temp::<State>(key)).unwrap_or((None, None, 0, false));
        // A parent menu that closed and opened again starts with no flyout.
        if seen + 1 < pass {
            (open, leave, moved) = (None, None, false);
        }
        // Only a menu that opened under a still pointer waits for a move; after that, any row the
        // pointer rests on shows its flyout, including once another flyout has closed.
        moved |= Self::pointer_moved(ctx);
        if open.is_none() && ((moved && row.hovered()) || row.clicked()) {
            open = Some(popup_id);
        }
        if open == Some(popup_id) {
            let pointer = ctx.pointer_hover_pos();
            let inside = pointer.is_some_and(|p| row.rect.contains(p) || ctx.read_response(popup_id).is_some_and(|r| r.rect.contains(p)));
            let elsewhere = pointer.is_some_and(|p| ui.clip_rect().contains(p)) && !inside;
            if !row.clicked() && elsewhere && (Self::pointer_moved(ctx) || leave.is_some()) {
                if Self::flyout_leaving(ctx, &mut leave) {
                    open = None;
                    // The row under the pointer may come earlier in this pass; it opens on the next.
                    ctx.request_repaint();
                }
            } else {
                leave = None;
            }
        }
        ctx.data_mut(|d| d.insert_temp(key, (open, leave, pass, moved)));
        open == Some(popup_id)
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

    pub fn show_frame<R>(ui: &mut Ui, frame: egui::Frame, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
        let bounds = ui.max_rect();
        Container::new(ui.ctx(), Surface::Popup).show_frame(ui, frame, bounds, |ui| {
            ui.visuals_mut().widgets.noninteractive.bg_stroke = Self::separator_stroke(&Tokens::get());
            add(ui)
        })
    }

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
        let (up, down, enter) =
            ctx.input(|i| (i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::Enter)));
        let usable: Vec<_> = enabled.iter().enumerate().filter_map(|(k, on)| on.then_some(k)).collect();
        if up || down {
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

    /// Place a submenu beside its row with [`Self::flyout_at`].
    pub fn submenu<R>(
        ctx: &egui::Context,
        id: egui::Id,
        row: Rect,
        height: f32,
        leftward: bool,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> egui::InnerResponse<R> {
        let width = LIST_W;
        let screen = ctx.content_rect();
        // The estimate only places the area's first, invisible sizing pass.
        let height = ctx.memory(|m| m.area_rect(id)).map_or(height, |r| r.height());
        let at = Self::flyout_at(screen, row, vec2(width, height), leftward);
        // The sizing pass caps the contents at the default size (egui's is 400 points high), and a
        // scrolling flyout keeps whatever height it measured there.
        Container::new(ctx, Surface::Popup)
            .area(id)
            .default_size(vec2(width, screen.height()))
            .fixed_pos(at)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| Self::show(ui, width, add))
    }
}

/// Shared row states. Swatches are document colour data; chrome always uses Tokens.
pub struct MenuItem<'a> {
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
}

impl<'a> MenuItem<'a> {
    pub fn new(id: &'a str, label: &'a str, icon: impl Into<Icon<'a>>) -> Self {
        Self {
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
        }
    }

    /// The row for a menu entry, `lit` when it is chosen with the keys or its flyout is open. A
    /// `current` shortcut marks the checked entry of a choice.
    pub fn from_item(item: &'a Item, lit: bool) -> Self {
        Self {
            id: &item.id,
            label: &item.label,
            icon: item.icon.as_icon(),
            shortcut: if item.shortcut == "current" { "" } else { &item.shortcut },
            enabled: item.enabled,
            highlighted: lit,
            danger: item.icon == "delete",
            checked: (item.shortcut == "current").then_some(true),
            swatch: item.swatch.map(|c| Color32::from_rgb(c[0], c[1], c[2])),
            submenu: !item.children.is_empty(),
        }
    }

    pub(crate) fn trailing_check_width(icon: Icon<'_>, checkable: bool) -> f32 {
        let t = Tokens::get();
        if !icon.is_blank() && checkable { t.space_16 + t.space_8 } else { 0.0 }
    }

    pub fn show(&self, ui: &mut Ui) -> Response {
        let t = Tokens::get();
        // Areas first measure intrinsic content. Filling their provisional available
        // width would make a flipped submenu cover its parent and consume the opening click.
        let width = if ui.is_sizing_pass() {
            let label =
                ui.painter().layout_no_wrap(solidcraft_ui_text::tr_label(self.label).to_string(), FontId::proportional(t.font_label), t.form_ink);
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
        let hot = enabled && (self.highlighted || response.hovered() || response.has_focus());
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
        let icon = Rect::from_center_size(pos2(r.left() + t.control_height / 2.0, r.center().y), vec2(t.space_16, t.space_16));
        if let Some(on) = self.checked.filter(|_| self.icon.is_blank()) {
            if on {
                super::checks::paint_check_glyph(&painter, icon, false, t.form_primary);
            }
        } else if let Some(color) = self.swatch {
            painter.rect(icon, t.radius_sm, color, t.control_border_stroke(t.control_border), egui::StrokeKind::Inside);
        } else if !self.icon.is_blank() {
            match self.icon {
                Icon::Kit(name) if self.danger => icons::paint_danger(&painter, icon, name, 1.0),
                Icon::Kit(name) if hot => icons::paint_ink(&painter, icon, name, dim),
                plain => plain.paint_plain(&painter, icon, 1.0),
            }
        }
        let label = solidcraft_ui_text::tr_label(self.label);
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
                super::checks::paint_check_glyph(&painter, check, false, t.form_primary);
            }
            right -= check_width;
        }
        if !self.shortcut.is_empty() {
            let key = painter.layout_no_wrap(solidcraft_ui_text::tr_label(self.shortcut).to_string(), FontId::monospace(t.font_small), dim);
            painter.galley(pos2(right - key.size().x, r.center().y - key.size().y / 2.0), key.clone(), dim);
            right -= key.size().x + t.space_8;
        }
        let left = r.left() + t.control_height + t.space_2;
        let mut job = egui::text::LayoutJob::simple_singleline(label.to_string(), FontId::proportional(t.font_label), ink);
        job.wrap.max_width = (right - left).max(0.0);
        job.wrap.max_rows = 1;
        let galley = painter.layout_job(job);
        painter.galley(pos2(left, r.center().y - galley.size().y / 2.0), galley, ink);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
        solidcraft_ui_registry::publish_target(&format!("menu:{}", self.id), r);
        if enabled && response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
}

/// A list menu's popup surface, [`LIST_W`] wide, around `add`.
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
    let bar = t.control_height;
    let view = (room - 2.0 * bar).max(bar);
    let end = (state.content - view).max(0.0);
    let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.1);
    let mut offset = state.offset.clamp(0.0, end);
    let up = scroll_bar(ui, id.with("up"), true, offset > 0.5);
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
    let down = scroll_bar(ui, id.with("down"), false, offset < end - 0.5);
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
fn scroll_bar(ui: &mut Ui, id: egui::Id, up: bool, active: bool) -> Response {
    let t = Tokens::get();
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.control_height), Sense::hover());
    let response = ui.interact(r, id, if active { Sense::click() } else { Sense::hover() });
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

/// How the workspace menu shows one of the host's workspaces: the slug of its handle
/// (`workspace:menu:<slug>`), the message ids of its description and of its tab list (`None`: a
/// planned workspace, not yet available) and its accent ramp (`None`: the main accent, tinted).
#[derive(Clone, Copy)]
pub struct WorkspaceStyle {
    pub slug: &'static str,
    pub description: &'static str,
    pub tabs: Option<&'static str>,
    pub ramp: Option<solidcraft_theme::AccentRamp>,
}

/// The host's workspace menu: its entries, how each is shown (an entry without a style is not
/// listed) and the footer note under them.
pub struct WorkspaceMenu<'a> {
    pub items: &'a [Item],
    pub style: &'a dyn Fn(&Item) -> Option<WorkspaceStyle>,
    pub footer: &'a str,
}

pub fn workspace_menu(ui: &mut egui::Ui, menu: &WorkspaceMenu<'_>, key_row: Option<usize>, height: f32) -> Option<Item> {
    let t = Tokens::get();
    let width = t.form_card_width + t.form_label_width * 2.0 + t.space_24 + t.space_16 + t.space_6;
    let mut picked = None;
    Menu::show_frame(ui, Menu::frame().inner_margin(t.space_8 + 1.0), |ui| {
        ui.set_width(width);
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add_space(t.space_4);
        ui.label(
            egui::RichText::new(solidcraft_ui_text::tr("ui.ribbon_workspace").to_uppercase())
                .size(solidcraft_theme::Tokens::get().font_small)
                .semibold()
                .color(t.text_dim),
        );
        ui.add_space(t.space_8);
        egui::ScrollArea::vertical().id_salt("workspace_menu_rows").max_height((height - 116.0).max(60.0)).show(ui, |ui| {
            for (k, item) in menu.items.iter().enumerate() {
                let Some(WorkspaceStyle { slug, description, tabs, ramp }) = (menu.style)(item) else { continue };
                let planned = tabs.is_none();
                let current = item.shortcut == "current";
                let (accent, soft) = match ramp.filter(|_| !planned) {
                    Some(ramp) => {
                        let accent = solidcraft_theme::TabAccent::for_ramp(&t, ramp);
                        (accent.color, accent.soft)
                    }
                    None => {
                        let accent = if planned { t.text_dim } else { t.accent };
                        (accent, t.panel.lerp_to_gamma(accent, 0.10))
                    }
                };
                let (r, resp) = ui.allocate_exact_size(
                    vec2(width, if planned { t.space_24 * 2.0 + t.space_12 } else { t.space_24 * 3.0 }),
                    if item.enabled { Sense::click() } else { Sense::hover() },
                );
                for handle in [format!("menu:{}", item.id), format!("workspace:menu:{slug}")] {
                    solidcraft_ui_registry::publish_target(&handle, r);
                }
                if current || (item.enabled && (resp.hovered() || key_row == Some(k))) {
                    ui.painter().rect_filled(r, t.control_radius.saturating_sub(1), soft);
                }
                ui.painter().rect_filled(
                    Rect::from_min_max(r.min + vec2(t.space_2, t.space_6 + 1.0), pos2(r.left() + 6.0, r.bottom() - 7.0)),
                    t.radius_sm / 2,
                    accent,
                );
                let tile = Rect::from_center_size(
                    pos2(r.left() + 40.0, r.center().y),
                    vec2(t.space_24 + t.space_16 + t.space_4, t.space_24 + t.space_16 + t.space_4),
                );
                ui.painter().rect_filled(tile, t.radius_xl.saturating_sub(1), if planned { t.panel_header } else { soft });
                let opacity = if item.enabled { 1.0 } else { 0.35 };
                let icon = item.icon.as_icon();
                if !icon.is_blank() {
                    icon.paint_plain(ui.painter(), tile.shrink(t.space_6 + 1.0), opacity);
                }
                let text = if item.enabled { t.text } else { t.text_dim };
                let name = solidcraft_ui_text::tr_label(&item.label);
                let x = r.left() + 74.0;
                let y = r.top() + 17.0;
                let galley =
                    egui::WidgetText::from(egui::RichText::new(name).semibold().size(solidcraft_theme::Tokens::get().font_heading).color(text))
                        .into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Button);
                let badge_x = x + galley.size().x + 9.0;
                let origin = pos2(x, y - galley.size().y / 2.0);
                let ppp = ui.ctx().pixels_per_point().max(0.5);
                let origin = pos2((origin.x * ppp).round() / ppp, (origin.y * ppp).round() / ppp);
                ui.painter().galley(origin, galley, text);
                let badge = solidcraft_ui_text::tr(if planned {
                    "ui.workspace_planned"
                } else if current {
                    "ui.workspace_current"
                } else {
                    "ui.workspace_available"
                });
                let badge = ui.painter().layout_no_wrap(
                    badge.to_string(),
                    FontId::proportional(solidcraft_theme::Tokens::get().font_small),
                    if current { t.on_accent } else { accent },
                );
                let badge_rect = Rect::from_min_size(pos2(badge_x, y - 8.0), badge.size() + vec2(t.space_12, t.space_3));
                ui.painter().rect_filled(badge_rect, t.radius_lg, if current { accent } else { soft });
                ui.painter().galley(badge_rect.min + vec2(t.space_6, t.space_3 / 2.0), badge, accent);
                let line = |content: String, y: f32| {
                    let mut job =
                        egui::text::LayoutJob::simple_singleline(content, FontId::proportional(solidcraft_theme::Tokens::get().font_body), text);
                    job.wrap.max_width = r.right() - x - 6.0;
                    job.wrap.max_rows = 1;
                    job.wrap.break_anywhere = false;
                    let galley = ui.painter().layout_job(job);
                    ui.painter().galley(pos2(x, y), galley, text);
                };
                line(solidcraft_ui_text::tr(description).to_string(), r.top() + 29.0);
                if let Some(tabs) = tabs {
                    // Translate each tab even when this composed message falls back to English.
                    let labels = solidcraft_ui_text::tr(tabs)
                        .split('·')
                        .map(|label| solidcraft_ui_text::tr_label(label.trim()))
                        .collect::<Vec<_>>()
                        .join(" · ");
                    line(labels, r.top() + 47.0);
                }
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, item.enabled, name));
                if resp.clicked() && item.enabled {
                    picked = Some(item.clone());
                }
            }
        });
        ui.add_space(t.space_2);
        Menu::separator(ui);
        ui.add_space(t.space_2);
        ui.add(egui::Label::new(egui::RichText::new(menu.footer).size(solidcraft_theme::Tokens::get().font_small).color(t.text_dim)).wrap());
        ui.add_space(t.space_4);
    });
    picked
}

/// Rename keeps the stable editor identity and selects the whole initial name.
pub fn rename_menu(ui: &mut Ui, value: &mut String, focused: &mut bool) -> Option<bool> {
    let t = Tokens::get();
    let mut done = None;
    Menu::show(ui, t.form_card_width, |ui| {
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
        assert_eq!(frame(key(egui::Key::Enter)), (Some(2), true));
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
