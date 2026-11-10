//! The only constructors for outer UI hosts. Content widgets belong to the other kit modules.
//! A surface declares its purpose; the shell supplies the layout once per frame. Never infer
//! floating/docked presentation from a panel name or keep a second layout preference here.
use crate::frame::Side;
use egui::{Color32, Context, Frame, Id, Stroke, WidgetText};
use serde::{Deserialize, Serialize};
use solidcraft_theme::Tokens;

pub type Background = fn(&egui::Painter, Frame, egui::Rect, egui::Shape) -> egui::Shape;

/// A reserved background slot for native hosts whose Frame is painted by egui.
pub(super) struct BackingSlot {
    painter: egui::Painter,
    slot: egui::layers::ShapeIdx,
}

impl BackingSlot {
    pub(super) fn reserve(ctx: &Context, layer: egui::LayerId) -> Option<Self> {
        if !crate::blur::enabled() || ctx.viewport_id() != egui::ViewportId::ROOT {
            return None;
        }
        let painter = ctx.layer_painter(layer);
        let slot = painter.add(egui::Shape::Noop);
        Some(Self { painter, slot })
    }

    pub(super) fn body<R>(&self, ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
        let frame = *ui.stack().frame();
        let painter = ui.painter().clone();
        let result = body(ui);
        self.painter.set(self.slot, crate::blur::decorate(&painter, frame, ui.min_rect(), egui::Shape::Noop));
        result
    }
}

/// Keep the stock combo's placement, focus and scroll behavior. Its popup frame is
/// reserved before `show_ui` paints it, just as for the other native popup hosts.
pub(super) fn show_combo<R>(
    ui: &mut egui::Ui,
    widget: Id,
    combo: egui::ComboBox,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    if !crate::blur::enabled() || ui.ctx().viewport_id() != egui::ViewportId::ROOT {
        return combo.show_ui(ui, body);
    }
    let popup = widget.with("popup");
    let slot = egui::ComboBox::is_open(ui.ctx(), widget)
        .then(|| BackingSlot::reserve(ui.ctx(), egui::LayerId::new(egui::Order::Foreground, popup)))
        .flatten();
    let mut host = None;
    let shown = combo.show_ui(ui, |ui| {
        // ComboBox calls us inside its ScrollArea, whose content Frame has no
        // fill and whose clip omits popup padding and any scrollbar. Its parent
        // owns the actual native popup Frame; finish using the final Area rect.
        if slot.is_some()
            && let Some(frame) = ui.stack().iter().find(|node| node.kind() == Some(egui::UiKind::ScrollArea)).and_then(|node| node.parent.as_ref())
        {
            let mut painter = ui.painter().clone();
            painter.set_clip_rect(ui.ctx().content_rect());
            host = Some((painter, *frame.frame()));
        }
        body(ui)
    });
    if let (Some(slot), Some((painter, frame)), Some(response)) = (slot, host, ui.ctx().read_response(popup)) {
        slot.painter.set(slot.slot, crate::blur::decorate(&painter, frame, response.rect - frame.total_margin(), egui::Shape::Noop));
    }
    shown
}

/// Native popup interaction and placement, with its backing surface reserved before contents.
pub struct BackingPopup<'a>(egui::Popup<'a>);

impl<'a> BackingPopup<'a> {
    pub fn frame(mut self, frame: Frame) -> Self {
        self.0 = self.0.frame(frame);
        self
    }
    pub fn close_behavior(mut self, value: egui::PopupCloseBehavior) -> Self {
        self.0 = self.0.close_behavior(value);
        self
    }
    pub fn width(mut self, value: f32) -> Self {
        self.0 = self.0.width(value);
        self
    }
    pub fn open(mut self, value: bool) -> Self {
        self.0 = self.0.open(value);
        self
    }
    pub fn open_memory(mut self, value: impl Into<Option<egui::SetOpenCommand>>) -> Self {
        self.0 = self.0.open_memory(value);
        self
    }
    pub fn at_pointer_fixed(mut self) -> Self {
        self.0 = self.0.at_pointer_fixed();
        self
    }
    pub fn anchor(mut self, value: impl Into<egui::PopupAnchor>) -> Self {
        self.0 = self.0.anchor(value);
        self
    }
    pub fn align(mut self, value: egui::RectAlign) -> Self {
        self.0 = self.0.align(value);
        self
    }
    pub fn align_alternatives(mut self, value: &'a [egui::RectAlign]) -> Self {
        self.0 = self.0.align_alternatives(value);
        self
    }
    pub fn gap(mut self, value: f32) -> Self {
        self.0 = self.0.gap(value);
        self
    }
    pub fn is_open(&self) -> bool {
        self.0.is_open()
    }
    pub fn get_expected_size(&self) -> Option<egui::Vec2> {
        self.0.get_expected_size()
    }
    pub fn show<R>(self, body: impl FnOnce(&mut egui::Ui) -> R) -> Option<egui::InnerResponse<R>> {
        let slot =
            self.0.is_open().then(|| BackingSlot::reserve(self.0.ctx(), egui::LayerId::new(egui::Order::Foreground, self.0.get_id()))).flatten();
        self.0.show(|ui| match slot {
            Some(slot) => slot.body(ui, body),
            None => body(ui),
        })
    }
}

pub struct BackingModal {
    modal: egui::Modal,
    id: Id,
}
impl BackingModal {
    pub fn backdrop_color(mut self, value: Color32) -> Self {
        self.modal = self.modal.backdrop_color(value);
        self
    }
    pub fn frame(mut self, value: Frame) -> Self {
        self.modal = self.modal.frame(value);
        self
    }
    pub fn show<R>(self, ctx: &Context, body: impl FnOnce(&mut egui::Ui) -> R) -> egui::ModalResponse<R> {
        if !crate::blur::enabled() || ctx.viewport_id() != egui::ViewportId::ROOT {
            return self.modal.show(ctx, body);
        }
        // Blur the preceding UI before native dimming, then blur the dialog's immediate
        // backdrop after dimming. Native Modal retains focus, input shielding and closing.
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, self.id));
        let backdrop = Frame::NONE.fill(self.modal.backdrop_color);
        painter.add(crate::blur::decorate(&painter, backdrop, ctx.content_rect(), egui::Shape::Noop));
        let frame = self.modal.frame.unwrap_or_else(|| Frame::popup(&ctx.global_style()));
        self.modal.frame(Frame::NONE).show(ctx, |ui| Container::show_backing(ui, frame, body).inner)
    }
}

/// Response tooltips retain native hover/disabled/stacking behavior and share backing blur.
pub trait BackingTooltipExt: Sized {
    fn backing_on_hover_ui(self, body: impl FnOnce(&mut egui::Ui)) -> Self;
    fn backing_on_disabled_hover_ui(self, body: impl FnOnce(&mut egui::Ui)) -> Self;
    fn backing_on_hover_ui_at_pointer(self, body: impl FnOnce(&mut egui::Ui)) -> Self;
    fn backing_on_hover_text(self, text: impl Into<WidgetText>) -> Self;
    fn backing_on_disabled_hover_text(self, text: impl Into<WidgetText>) -> Self;
    fn backing_on_hover_text_at_pointer(self, text: impl Into<WidgetText>) -> Self;
    fn backing_show_tooltip_ui(&self, body: impl FnOnce(&mut egui::Ui));
    fn backing_show_tooltip_text(&self, text: impl Into<WidgetText>);
}

/// What egui's `on_hover_text` does besides showing the tip: a widget that has an accessibility
/// node gets the tip as its description, and as its name when it has none (a label is named by
/// its text, a widget with a `labelled_by` relation by that). Mirrors egui's private
/// `Response::describe_for_accessibility`, so a converted caller reads the same to a screen reader.
fn describe_for_accessibility(response: &egui::Response, text: &str) {
    if response.ctx.has_accesskit_node(response.id) {
        response.ctx.accesskit_node_builder(response.id, |node| {
            if node.description().is_none() {
                node.set_description(text);
            }
            if node.role() != egui::accesskit::Role::Label && node.labelled_by().is_empty() && node.label().is_none_or(str::is_empty) {
                node.set_label(text);
            }
        });
    }
}

fn show_backing_tooltip(parent: Id, tip: egui::Tooltip<'_>, body: impl FnOnce(&mut egui::Ui)) {
    let slot = tip
        .popup
        .is_open()
        .then(|| {
            BackingSlot::reserve(tip.popup.ctx(), egui::LayerId::new(egui::Order::Tooltip, egui::Tooltip::next_tooltip_id(tip.popup.ctx(), parent)))
        })
        .flatten();
    tip.show(|ui| match slot {
        Some(slot) => slot.body(ui, body),
        None => body(ui),
    });
}

impl BackingTooltipExt for egui::Response {
    fn backing_on_hover_ui(self, body: impl FnOnce(&mut egui::Ui)) -> Self {
        show_backing_tooltip(self.id, egui::Tooltip::for_enabled(&self), body);
        self
    }
    fn backing_on_disabled_hover_ui(self, body: impl FnOnce(&mut egui::Ui)) -> Self {
        show_backing_tooltip(self.id, egui::Tooltip::for_disabled(&self), body);
        self
    }
    fn backing_on_hover_ui_at_pointer(self, body: impl FnOnce(&mut egui::Ui)) -> Self {
        show_backing_tooltip(self.id, egui::Tooltip::for_enabled(&self).at_pointer().gap(12.0), body);
        self
    }
    fn backing_on_hover_text(self, text: impl Into<WidgetText>) -> Self {
        let text = text.into();
        describe_for_accessibility(&self, text.text());
        self.backing_on_hover_ui(|ui| {
            ui.set_max_width(ui.spacing().tooltip_width);
            ui.label(text);
        })
    }
    fn backing_on_disabled_hover_text(self, text: impl Into<WidgetText>) -> Self {
        self.backing_on_disabled_hover_ui(|ui| {
            ui.set_max_width(ui.spacing().tooltip_width);
            ui.label(text);
        })
    }
    fn backing_on_hover_text_at_pointer(self, text: impl Into<WidgetText>) -> Self {
        let text = text.into();
        describe_for_accessibility(&self, text.text());
        self.backing_on_hover_ui_at_pointer(|ui| {
            ui.set_max_width(ui.spacing().tooltip_width);
            ui.label(text);
        })
    }
    fn backing_show_tooltip_ui(&self, body: impl FnOnce(&mut egui::Ui)) {
        let popup = egui::Popup::from_response(self).kind(egui::PopupKind::Tooltip);
        let slot = BackingSlot::reserve(&self.ctx, egui::LayerId::new(egui::Order::Tooltip, popup.get_id()));
        popup.show(|ui| match slot {
            Some(slot) => slot.body(ui, body),
            None => body(ui),
        });
    }
    fn backing_show_tooltip_text(&self, text: impl Into<WidgetText>) {
        self.backing_show_tooltip_ui(|ui| {
            ui.label(text);
        });
    }
}

const MAX_LAYER_RAISES_PER_PASS: usize = 256;
const MAX_HOST_WINDOWS_PER_PASS: usize = 256;

#[derive(Clone, Default)]
struct HostWindows {
    stamp: (u64, u64),
    windows: Vec<(Id, egui::Rect)>,
    exhausted: bool,
}

fn host_windows_key(ctx: &Context) -> Id {
    Id::unique(("solidcraft_host_windows", ctx.viewport_id()))
}

#[derive(Clone, Default)]
struct LayerRaises {
    stamp: (egui::ViewportId, u64, u64),
    layers: Vec<egui::LayerId>,
    exhausted: bool,
}

fn layer_raise_key(ctx: &Context) -> Id {
    Id::unique(("solidcraft_container_layer_raises", ctx.viewport_id()))
}

#[derive(Clone)]
pub struct ModalBoundary {
    pub stamp: (u64, u64),
}

#[derive(Clone)]
struct HostEscape {
    stamp: (u64, u64),
    event: egui::Event,
}

fn modal_boundary_key(ctx: &Context) -> Id {
    Id::unique(("solidcraft_container_modal", ctx.viewport_id()))
}
fn host_escape_key(ctx: &Context) -> Id {
    Id::unique(("solidcraft_container_host_escape", ctx.viewport_id()))
}

/// The shared modal show boundary, including its first invisible sizing pass.
/// Constructing a modal alone never changes the input context.
#[must_use]
pub struct ModalHost {
    modal: BackingModal,
}

impl ModalHost {
    pub fn backdrop_color(mut self, color: Color32) -> Self {
        self.modal = self.modal.backdrop_color(color);
        self
    }

    pub fn frame(mut self, frame: Frame) -> Self {
        self.modal = self.modal.frame(frame);
        self
    }

    pub fn show<T>(self, ctx: &Context, content: impl FnOnce(&mut egui::Ui) -> T) -> egui::ModalResponse<T> {
        let stamp = (ctx.cumulative_frame_nr(), ctx.cumulative_pass_nr());
        let modal_key = modal_boundary_key(ctx);
        let escape_key = host_escape_key(ctx);
        let reserved = ctx.data_mut(|data| {
            data.insert_temp(modal_key, ModalBoundary { stamp });
            let reserved = data.get_temp::<HostEscape>(escape_key);
            data.remove::<HostEscape>(escape_key);
            reserved.filter(|reserved| reserved.stamp == stamp)
        });
        // A modal can open after logic reserved Escape for a panel gesture.
        // Transfer the original event once to the modal, without replaying it
        // to ordinary windows or changing the pending panel layout state.
        if let Some(reserved) = reserved {
            ctx.input_mut(|input| {
                if !input.key_pressed(egui::Key::Escape) {
                    input.events.push(reserved.event);
                }
            });
        }
        self.modal.show(ctx, content)
    }
}

/// The feature sheet's deliberately restricted stock Window (no title, scrolling or fades).
/// Keep its builder private: the marker's layer and host clip are owned here.
pub struct FeatureWindow<'a> {
    window: egui::Window<'a>,
    id: Id,
    frame: Frame,
    open: Option<bool>,
}

impl<'a> FeatureWindow<'a> {
    pub fn new(window: egui::Window<'a>, id: Id, frame: Frame) -> Self {
        Self { window, id, frame, open: None }
    }

    pub fn fixed_size(mut self, size: egui::Vec2) -> Self {
        self.window = self.window.fixed_size(size);
        self
    }

    /// Borrow a caller's open flag only for this showing, retaining stock close handling.
    pub fn open<'open>(self, open: &'open mut bool) -> FeatureWindow<'open>
    where
        'a: 'open,
    {
        let is_open = *open;
        let window: egui::Window<'open> = self.window;
        FeatureWindow { window: window.open(open), id: self.id, frame: self.frame, open: Some(is_open) }
    }

    pub fn show<R>(self, ctx: &Context, body: impl FnOnce(&mut egui::Ui) -> R) -> Option<egui::InnerResponse<Option<R>>> {
        let background = Container::floating_card(Surface::Feature).background(ctx, self.frame);
        self.show_background(ctx, background, body)
    }

    pub fn show_background<R>(
        self,
        ctx: &Context,
        background: Option<Background>,
        body: impl FnOnce(&mut egui::Ui) -> R,
    ) -> Option<egui::InnerResponse<Option<R>>> {
        let backing = background.map(|background| {
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, self.id));
            let slot = painter.add(egui::Shape::Noop);
            (background, slot)
        });
        // Window::show animates its open flag even with fade_out(false).
        // A closed feature must disappear before its body or footprint is produced.
        if self.open == Some(false) {
            Container::record_host_window(ctx, self.id, None);
            return None;
        }
        let Some((background, slot)) = backing else {
            let mut painted = false;
            let shown = self.window.show(ctx, |ui| {
                painted = ui.is_visible() && !ui.is_sizing_pass();
                body(ui)
            });
            let bounds = shown
                .as_ref()
                .filter(|shown| {
                    painted
                        || shown.inner.is_none()
                            && egui::containers::AreaState::load(ctx, shown.response.layer_id.id).is_some_and(|area| area.interactable)
                })
                .map(|shown| shown.response.rect);
            Container::record_host_window(ctx, self.id, bounds);
            return shown;
        };
        let mut host_painter = None;
        let mut painted = false;
        let shown = self.window.show(ctx, |ui| {
            painted = ui.is_visible() && !ui.is_sizing_pass();
            // Resize's content clip omits the frame border/padding. This bounded Window
            // uses Area's default viewport clip, with no fades or native title bar.
            let mut painter = ui.painter().clone();
            painter.set_clip_rect(ctx.content_rect());
            host_painter = Some(painter);
            body(ui)
        });
        let bounds = shown
            .as_ref()
            .filter(|shown| {
                painted
                    || shown.inner.is_none()
                        && egui::containers::AreaState::load(ctx, shown.response.layer_id.id).is_some_and(|area| area.interactable)
            })
            .map(|shown| shown.response.rect);
        Container::record_host_window(ctx, self.id, bounds);
        if let (Some(shown), Some(painter)) = (&shown, host_painter) {
            // Window relocates inner_margin into an internal content Frame. Its final
            // Area response still includes all margins and the original stroke width.
            let content = shown.response.rect - self.frame.total_margin();
            if painter.is_visible() && painter.clip_rect().intersects(self.frame.widget_rect(content)) {
                painter.set(slot, background(&painter, self.frame, content, egui::Shape::Noop));
            }
        }
        shown
    }
}

/// Persistent panels adapt to the layout; transient UI retains its anchoring and modality.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// Browser, History, Render settings and future workspace panels.
    Panel,
    /// Feature forms and the Assistant sheet.
    Feature,
    /// Sketch options, including their translucent background.
    Palette,
    /// Workspace switcher and ribbon cards.
    Chrome,
    /// Native application/title bar and the optional status bar along the window's bottom edge;
    /// their presentation is independent of workspace layout.
    ApplicationBar,
    /// Movable system windows and modal dialogs; never become workspace docks.
    Window,
    /// Anchored menus, completion lists and rename editors.
    Popup,
    /// Model-anchored editors and manipulation feedback.
    Canvas,
    /// Status/progress notifications, independent of the workspace layout.
    Toast,
    /// Viewport navigation controls (the compass itself is drawn geometry).
    Navigation,
    /// Persistent, content-hugging status pills above the free model viewport.
    ViewportStatus,
    /// Full-page content, the model viewport and native pop-out content.
    Viewport,
    /// Deliberate input-blocking backdrops; must not inherit CAD input blocking.
    InputShield,
}

#[derive(Clone, Copy, Debug)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// Edges that meet the window or another docked surface. Only exposed corners round.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DockEdges {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

impl DockEdges {
    pub const ALL: Self = Self { left: true, right: true, top: true, bottom: true };
}

/// Saved workspace layout; automation may override it for a session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LayoutMode {
    #[default]
    Floating,
    #[serde(rename = "compact", alias = "docked")]
    Docked,
}

impl LayoutMode {
    pub fn publish(self, ctx: &egui::Context) {
        ctx.data_mut(|data| data.insert_temp(egui::Id::unique("sc_container_layout"), self));
    }

    pub fn current(ctx: &egui::Context) -> Self {
        ctx.data(|data| data.get_temp(egui::Id::unique("sc_container_layout"))).unwrap_or_default()
    }
}

/// The gap from the viewport edge to the floating cards and between the cards.
pub const SHELL_GAP: f32 = 7.0;

thread_local! {
    /// The areas built as [`Surface::Canvas`]: a value box, a dimension's editor, a readout.
    static CANVAS_AREAS: std::cell::RefCell<std::collections::HashSet<Id>> = std::cell::RefCell::default();
    /// Set by the host while something else owns the input over the CAD view (a reviewed issue
    /// report, a native file picker): kit surfaces other than [`Surface::InputShield`] take none.
    static INPUT_BLOCKED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Does the host hold the CAD input back this frame? See [`set_input_blocked`].
pub fn input_blocked() -> bool {
    INPUT_BLOCKED.with(std::cell::Cell::get)
}

/// The host marks the CAD input as blocked, or no longer blocked.
pub fn set_input_blocked(blocked: bool) {
    INPUT_BLOCKED.with(|cell| cell.set(blocked));
}

/// Is `layer` a model-anchored editor or readout ([`Surface::Canvas`])? Such an area sits on the
/// model and scrolls nothing, so the model's wheel and trackpad navigation go on under it (#10061).
pub fn is_canvas(layer: egui::LayerId) -> bool {
    CANVAS_AREAS.with(|areas| areas.borrow().contains(&layer.id))
}

/// Universal outer host and surface policy. Both host geometry and frame construction must go
/// through this API, including code in nested modules and platform apps.
#[derive(Clone, Copy)]
pub struct Container {
    surface: Surface,
    layout: LayoutMode,
    joined: Option<DockEdges>,
}

impl Container {
    pub fn new(ctx: &Context, surface: Surface) -> Self {
        Self::for_layout(LayoutMode::current(ctx), surface)
    }

    pub fn for_layout(layout: LayoutMode, surface: Surface) -> Self {
        Self { surface, layout, joined: None }
    }

    /// Floating workspace cards retain their shared chrome even in compact ribbon layouts.
    pub fn floating_card(surface: Surface) -> Self {
        Self::for_layout(LayoutMode::Floating, surface)
    }

    /// Shared bottom-strip placement, used by History and other workspace strips.
    pub fn bottom_strip_rect(self, available: egui::Rect, height: f32) -> egui::Rect {
        let inset = available.shrink(self.gap());
        egui::Rect::from_min_max(egui::pos2(inset.left(), (inset.bottom() - height).max(inset.top())), inset.max)
    }

    /// Anchored navigation surfaces share the compass/nav-bar frame and token padding.
    pub fn navigation_card<R>(
        ctx: &Context,
        id: Id,
        anchor: egui::Pos2,
        pivot: egui::Align2,
        add: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::InnerResponse<R> {
        let container = Self::new(ctx, Surface::Navigation);
        container.area(id).movable(false).pivot(pivot).fixed_pos(anchor).order(egui::Order::Middle).show(ctx, |ui| {
            let frame = container.frame(ctx, Tokens::get().overlay, Side::Left).inner_margin(Tokens::get().space_4);
            container.show_frame(ui, frame, ctx.content_rect(), |ui| super::scope(ui, add)).inner
        })
    }

    /// Reserve an anchored one-row Navigation card and the common card gap.
    pub fn below_navigation_row(bounds: egui::Rect) -> egui::Rect {
        let t = Tokens::get();
        egui::Rect::from_min_max(egui::pos2(bounds.left(), (bounds.top() + t.control_height + t.space_8 * 2.0).min(bounds.bottom())), bounds.max)
    }

    /// Inline navigation band using the same chrome and padding as anchored HUDs.
    pub fn navigation_contents<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
        let container = Self::new(ui.ctx(), Surface::Navigation);
        let frame = container.frame(ui.ctx(), Tokens::get().overlay, Side::Left).inner_margin(Tokens::get().space_4);
        container.show_frame(ui, frame, ui.max_rect(), |ui| super::scope(ui, add))
    }

    /// Compact selection actions above canvas geometry, using the navigation host policy.
    pub fn selection_card<R>(
        ctx: &Context,
        id: Id,
        anchor: egui::Pos2,
        bounds: egui::Rect,
        add: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::InnerResponse<R> {
        let container = Self::new(ctx, Surface::Navigation);
        let t = Tokens::get();
        container.area(id).movable(false).pivot(egui::Align2::CENTER_BOTTOM).fixed_pos(anchor).constrain_to(bounds).order(egui::Order::Middle).show(
            ctx,
            |ui| {
                let frame =
                    container.frame(ctx, t.dialog_body, Side::Left).corner_radius(t.space_24 as u8).shadow(t.shell_shadow()).inner_margin(t.space_3);
                container.show_frame(ui, frame, bounds, |ui| super::scope(ui, add)).inner
            },
        )
    }

    /// Standard content tooltip, including the same host and padding as library samples.
    pub fn show_tooltip<R>(parent: Id, tip: egui::Tooltip<'_>, body: impl FnOnce(&mut egui::Ui) -> R) -> Option<egui::InnerResponse<R>> {
        let slot = tip
            .popup
            .is_open()
            .then(|| {
                BackingSlot::reserve(
                    tip.popup.ctx(),
                    egui::LayerId::new(egui::Order::Tooltip, egui::Tooltip::next_tooltip_id(tip.popup.ctx(), parent)),
                )
            })
            .flatten();
        tip.show(|ui| match slot {
            Some(slot) => slot.body(ui, body),
            None => body(ui),
        })
    }

    pub fn tooltip(response: &egui::Response, text: &str) {
        let ctx = &response.ctx;
        let parent = response.id;
        let mut tip = egui::Tooltip::for_enabled(response).at_pointer();
        tip.popup = tip.popup.frame(Self::new(ctx, Surface::Toast).tooltip_frame(ctx));
        let slot = tip
            .popup
            .is_open()
            .then(|| BackingSlot::reserve(ctx, egui::LayerId::new(egui::Order::Tooltip, egui::Tooltip::next_tooltip_id(tip.popup.ctx(), parent))))
            .flatten();
        tip.show(|ui| {
            let body = |ui: &mut egui::Ui| {
                ui.label(text);
                solidcraft_ui_registry::publish_rect("kit:tooltip", ui.min_rect());
            };
            match slot {
                Some(slot) => slot.body(ui, body),
                None => body(ui),
            }
        });
    }

    /// [`Self::tooltip`] holding what `add` draws (a picture, say), beside `response` rather than
    /// at the pointer, so it does not cover the row's neighbours.
    pub fn tooltip_ui(response: &egui::Response, add: impl FnOnce(&mut egui::Ui)) {
        let ctx = &response.ctx;
        let parent = response.id;
        let mut tip = egui::Tooltip::for_enabled(response);
        tip.popup = tip.popup.align(egui::RectAlign::RIGHT_START).frame(Self::new(ctx, Surface::Toast).tooltip_frame(ctx));
        let slot = tip
            .popup
            .is_open()
            .then(|| BackingSlot::reserve(ctx, egui::LayerId::new(egui::Order::Tooltip, egui::Tooltip::next_tooltip_id(tip.popup.ctx(), parent))))
            .flatten();
        tip.show(|ui| {
            let body = |ui: &mut egui::Ui| {
                add(ui);
                solidcraft_ui_registry::publish_rect("kit:tooltip", ui.min_rect());
            };
            match slot {
                Some(slot) => slot.body(ui, body),
                None => body(ui),
            }
        });
    }

    /// Declare contacts from shell geometry; styling remains entirely in the container.
    pub fn joined(mut self, edges: DockEdges) -> Self {
        self.joined = Some(edges);
        self
    }

    /// Shell-owned placement policy, shared by current and future adaptive panels.
    pub fn floating(self) -> bool {
        matches!(self.surface, Surface::Panel | Surface::Feature | Surface::Palette | Surface::Chrome) && self.layout == LayoutMode::Floating
    }

    /// Separation between adaptive workspace surfaces; compact surfaces meet their edge.
    pub fn gap(self) -> f32 {
        if self.floating() { SHELL_GAP } else { 0.0 }
    }

    /// Tokenised outer chrome. Callers may supply content padding, not another card surface.
    pub fn frame(self, ctx: &Context, fill: Color32, side: crate::frame::Side) -> Frame {
        let fill = if self.adaptive() { crate::blur::panel_fill(fill) } else { fill };
        let t = Tokens::get();
        let flat = Frame::NONE.fill(fill);
        match self.surface {
            Surface::Feature | Surface::Palette if !self.floating() => {
                let edges = self.joined.unwrap_or(DockEdges { left: side == Side::Left, right: side == Side::Right, ..Default::default() });
                Self::compact_frame(fill, t.primary_border(t.dialog_border), edges)
            }
            Surface::Panel | Surface::Chrome if !self.floating() => {
                Self::compact_frame(fill, t.primary_border(t.border), self.joined.unwrap_or(DockEdges::ALL))
            }
            Surface::Feature | Surface::Window => flat.stroke(t.primary_border(t.dialog_border)).corner_radius(t.radius_xl).shadow(t.shell_shadow()),
            Surface::Palette => flat.stroke(t.primary_border(t.border)).corner_radius(t.radius_xl).shadow(t.shell_shadow()),
            Surface::Panel | Surface::Chrome => flat.stroke(t.primary_border(t.border)).corner_radius(t.radius_lg).shadow(t.shell_shadow()),
            Surface::Navigation => flat.stroke(t.primary_border(t.border)).corner_radius(t.radius_md),
            Surface::ViewportStatus => flat.stroke(t.primary_border(t.border)).corner_radius((t.control_height / 2.0 + t.space_4) as u8),
            Surface::Toast => flat.stroke(t.primary_border(t.border)).corner_radius(t.radius_md).shadow(t.shell_shadow()),
            Surface::Popup => Self::popup_frame(&ctx.global_style()).fill(fill),
            Surface::ApplicationBar | Surface::Canvas | Surface::Viewport | Surface::InputShield => flat,
        }
    }

    /// Movable tool sheets round all exposed corners, joining only their right dock edge (or the
    /// edges they were declared `joined` at).
    pub fn tool_frame(self, mut frame: Frame, right_docked: bool) -> Frame {
        if self.surface == Surface::Feature && !self.floating() {
            frame.corner_radius = Self::compact_rounding(self.joined.unwrap_or(DockEdges { right: right_docked, ..Default::default() }));
        }
        frame
    }

    /// Surface of an allocated dock or internal pinned band. Floating card decoration is
    /// supplied by `frame`; edge-hosted panes must not acquire outer shadows or card gaps.
    pub fn edge_frame(self, fill: Color32) -> Frame {
        let fill = if self.adaptive() { crate::blur::panel_fill(fill) } else { fill };
        if self.adaptive() && !self.floating() {
            Self::compact_frame(fill, Tokens::get().primary_border(Tokens::get().border), self.joined.unwrap_or(DockEdges::ALL))
        } else {
            Frame::NONE.fill(fill)
        }
    }

    fn adaptive(self) -> bool {
        matches!(self.surface, Surface::Panel | Surface::Feature | Surface::Palette | Surface::Chrome)
    }

    fn compact_rounding(edges: DockEdges) -> egui::CornerRadius {
        let radius = Tokens::get().radius_lg;
        egui::CornerRadius {
            nw: if edges.left || edges.top { 0 } else { radius },
            ne: if edges.right || edges.top { 0 } else { radius },
            sw: if edges.left || edges.bottom { 0 } else { radius },
            se: if edges.right || edges.bottom { 0 } else { radius },
        }
    }

    fn compact_frame(fill: Color32, border: Stroke, edges: DockEdges) -> Frame {
        // Collapse adjoining borders onto the same themed-width strip. North/west outlines
        // sit outside the allocation; south/east outlines sit inside it. Logical panel
        // rectangles still touch exactly, with neither gaps nor doubled strokes.
        Frame::NONE
            .fill(fill)
            .stroke(border)
            .outer_margin(egui::Margin { left: -(border.width.ceil() as i8), top: -(border.width.ceil() as i8), right: 0, bottom: 0 })
            .corner_radius(Self::compact_rounding(edges))
    }

    /// Measure content before rounding a compact card. Reaching a dock boundary joins
    /// that edge, so a content-hugging card and a full-height card share one composition.
    pub fn show_frame<R>(self, ui: &mut egui::Ui, frame: Frame, bounds: egui::Rect, body: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
        let background = self.background(ui.ctx(), frame);
        self.show_frame_background(ui, frame, bounds, background, body)
    }

    /// Stock Frame path for backing cards; no additional inset chrome is introduced.
    pub fn show_backing<R>(ui: &mut egui::Ui, frame: Frame, body: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
        if !crate::blur::enabled()
            || frame.fill.a() == 0
            || (frame.fill.a() == 255 && ui.painter().opacity() >= 1.0)
            || ui.ctx().viewport_id() != egui::ViewportId::ROOT
        {
            return frame.show(ui, body);
        }
        let slot = ui.painter().add(egui::Shape::Noop);
        let mut prepared = frame.begin(ui);
        let inner = body(&mut prepared.content_ui);
        ui.painter().set(slot, crate::blur::decorate(ui.painter(), frame, prepared.content_ui.min_rect(), egui::Shape::Noop));
        egui::InnerResponse { inner, response: prepared.end(ui) }
    }

    /// Fill an already-reserved backing slot, preserving its original paint ordering.
    pub fn set_backing(painter: &egui::Painter, slot: egui::layers::ShapeIdx, frame: Frame, content: egui::Rect) {
        let shape = frame.paint(content);
        painter.set(slot, if crate::blur::enabled() { crate::blur::decorate(painter, frame, content, shape) } else { shape });
    }

    pub fn show_central<R>(
        ui: &mut egui::Ui,
        panel: egui::CentralPanel,
        frame: Frame,
        body: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::InnerResponse<R> {
        if !crate::blur::enabled()
            || frame.fill.a() == 0
            || (frame.fill.a() == 255 && ui.painter().opacity() >= 1.0)
            || ui.ctx().viewport_id() != egui::ViewportId::ROOT
        {
            return panel.frame(frame).show(ui, body);
        }
        let slot = BackingSlot::reserve(ui.ctx(), ui.layer_id());
        panel.frame(frame).show(ui, |ui| match slot {
            Some(slot) => slot.body(ui, body),
            None => body(ui),
        })
    }

    fn background(self, ctx: &Context, frame: Frame) -> Option<Background> {
        // Keep the off path to boundary checks only: no painter, shape slot or allocation.
        if crate::blur::enabled() && ctx.viewport_id() == egui::ViewportId::ROOT && frame.fill.a() > 0 { Some(crate::blur::decorate) } else { None }
    }

    pub(super) fn show_frame_background<R>(
        self,
        ui: &mut egui::Ui,
        frame: Frame,
        bounds: egui::Rect,
        background: Option<Background>,
        body: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::InnerResponse<R> {
        let slot = background.map(|_| ui.painter().add(egui::Shape::Noop));
        let mut prepared = frame.begin(ui);
        let inner = body(&mut prepared.content_ui);
        if self.adaptive() && !self.floating() {
            let rect = prepared.frame.outer_rect(prepared.content_ui.min_rect());
            let mut edges = self.joined.unwrap_or(DockEdges::ALL);
            edges.left |= rect.left() <= bounds.left();
            edges.right |= rect.right() >= bounds.right();
            edges.top |= rect.top() <= bounds.top();
            edges.bottom |= rect.bottom() >= bounds.bottom();
            prepared.frame.corner_radius = Self::compact_rounding(edges);
        }
        let frame = prepared.frame;
        if let (Some(background), Some(slot)) = (background, slot)
            && ui.is_rect_visible(frame.widget_rect(prepared.content_ui.min_rect()))
        {
            let painter = ui.painter();
            painter.set(slot, background(painter, frame, prepared.content_ui.min_rect(), egui::Shape::Noop));
        }
        let response = prepared.end(ui);
        Self::paint_inset(ui.painter(), response.rect - frame.outer_margin, frame);
        egui::InnerResponse { inner, response }
    }

    /// Optional companion to a primary outline, painted inside the allocated surface.
    /// It consumes no layout space or input target. Tiny/collapsed surfaces omit it.
    pub fn paint_inset(painter: &egui::Painter, widget: egui::Rect, frame: Frame) {
        // Ordinary hosts supply a content well clip, which excludes their padding.
        // Their companion belongs to the whole surface instead of that well.
        Self::paint_inset_clipped(painter, widget, frame, painter.ctx().content_rect());
    }

    /// Bounded hosts supply their inherited outer clip so decoration cannot
    /// escape a partially visible card. Geometry matches the ordinary companion.
    pub fn paint_inset_clipped(painter: &egui::Painter, widget: egui::Rect, frame: Frame, outer_clip: egui::Rect) {
        let mut painter = painter.clone();
        painter.set_clip_rect(widget.intersect(painter.ctx().content_rect()).intersect(outer_clip));
        let t = Tokens::get();
        if let Some(bevel) = t.borders.bevel {
            bevel.paint(&painter, widget, frame.corner_radius, false, true);
        }
        let Some(inset) = t.borders.primary_inset else {
            return;
        };
        let distance = frame.stroke.width + inset.gap;
        let rect = widget.shrink(distance);
        if inset.stroke.width <= 0.0 || !rect.is_finite() || rect.width() <= 2.0 * inset.stroke.width || rect.height() <= 2.0 * inset.stroke.width {
            return;
        }
        let reduce = |r: u8| r.saturating_sub(distance.ceil() as u8);
        let radius = frame.corner_radius;
        painter.rect_stroke(
            rect,
            egui::CornerRadius { nw: reduce(radius.nw), ne: reduce(radius.ne), sw: reduce(radius.sw), se: reduce(radius.se) },
            inset.stroke,
            egui::StrokeKind::Inside,
        );
    }

    /// Manually painted chrome uses exactly the same outer and inset policy as hosts.
    pub fn paint_frame(painter: &egui::Painter, frame: Frame, content: egui::Rect) {
        let shape = frame.paint(content);
        painter.add(if crate::blur::enabled() { crate::blur::decorate(painter, frame, content, shape) } else { shape });
        Self::paint_inset(painter, frame.widget_rect(content), frame);
    }

    /// Semantic manually painted workspace surface; HUDs keep `paint_frame`.
    pub fn paint_background(self, painter: &egui::Painter, frame: Frame, content: egui::Rect) {
        let shape = frame.paint(content);
        painter.add(if crate::blur::enabled() { crate::blur::decorate(painter, frame, content, shape) } else { shape });
    }

    pub fn paint_panel_frame(painter: &egui::Painter, frame: Frame, content: egui::Rect) {
        Self::floating_card(Surface::Panel).paint_background(painter, frame, content);
        Self::paint_inset(painter, frame.widget_rect(content), frame);
    }

    /// Decorate the content-hugging frame of a popup or feature sheet after its contents.
    pub fn finish_content(ui: &egui::Ui, frame: Frame) {
        Self::paint_inset(ui.painter(), frame.widget_rect(ui.min_rect()), frame);
    }

    pub fn area(self, id: Id) -> egui::Area {
        if self.surface == Surface::Canvas {
            CANVAS_AREAS.with(|areas| areas.borrow_mut().insert(id));
        }
        // Workspace cards and viewport status stay below tool/system windows even when
        // clicked. Other areas keep the normal window layer; popups choose their order.
        let order = if matches!(self.surface, Surface::Panel | Surface::ViewportStatus) { egui::Order::Background } else { egui::Order::Middle };
        egui::Area::new(id).enabled(self.input_enabled()).order(order)
    }

    /// Start collection only for the opt-in host, isolated to this native
    /// viewport and egui pass. Ordinary shared windows keep their old behavior.
    pub fn begin_host_windows(ctx: &Context) {
        let stamp = (ctx.cumulative_frame_nr(), ctx.cumulative_pass_nr());
        let key = host_windows_key(ctx);
        ctx.data_mut(|data| data.insert_temp(key, HostWindows { stamp, ..Default::default() }));
    }

    /// Persistent Feature/Window wrappers declare actual paint here. Popup,
    /// Modal and model-following Area wrappers never enter this ledger.
    pub fn record_host_window(ctx: &Context, id: Id, bounds: Option<egui::Rect>) {
        let stamp = (ctx.cumulative_frame_nr(), ctx.cumulative_pass_nr());
        let key = host_windows_key(ctx);
        ctx.data_mut(|data| {
            let Some(mut windows) = data.get_temp::<HostWindows>(key).filter(|windows| windows.stamp == stamp) else {
                return;
            };
            if let Some(bounds) = bounds {
                if !bounds.is_positive() || !bounds.is_finite() {
                    windows.exhausted = true;
                } else if let Some(entry) = windows.windows.iter_mut().find(|(existing, _)| *existing == id) {
                    entry.1 = bounds;
                } else if windows.windows.len() < MAX_HOST_WINDOWS_PER_PASS {
                    windows.windows.push((id, bounds));
                } else {
                    windows.exhausted = true;
                }
            } else {
                windows.windows.retain(|(existing, _)| *existing != id);
            }
            data.insert_temp(key, windows);
        });
    }

    /// None refuses framing/control placement when the bounded ledger failed.
    pub fn host_window_regions(ctx: &Context) -> Option<Vec<egui::Rect>> {
        let stamp = (ctx.cumulative_frame_nr(), ctx.cumulative_pass_nr());
        let key = host_windows_key(ctx);
        match ctx.data(|data| data.get_temp::<HostWindows>(key)) {
            Some(windows) if windows.stamp == stamp => (!windows.exhausted).then(|| windows.windows.into_iter().map(|(_, bounds)| bounds).collect()),
            _ => Some(Vec::new()),
        }
    }

    /// Raise an outer host through the shared kit. egui applies raises at the
    /// end of the pass; deferred host actions also need to observe that intent.
    /// Raised layers form a set: egui preserves their existing relative order.
    pub fn bring_to_front(ctx: &Context, layer: egui::LayerId) {
        let stamp = (ctx.viewport_id(), ctx.cumulative_frame_nr(), ctx.cumulative_pass_nr());
        let key = layer_raise_key(ctx);
        ctx.data_mut(|data| {
            let mut raises = data.get_temp::<LayerRaises>(key).unwrap_or_default();
            if raises.stamp != stamp {
                raises = LayerRaises { stamp, ..Default::default() };
            }
            if !raises.layers.contains(&layer) {
                if raises.layers.len() < MAX_LAYER_RAISES_PER_PASS {
                    raises.layers.push(layer);
                } else {
                    raises.exhausted = true;
                }
            }
            data.insert_temp(key, raises);
        });
        ctx.move_to_top(layer);
    }

    /// None means the bounded raise ledger overflowed; consumers refuse their
    /// own pending actions rather than inventing a viewport-wide input shield.
    pub fn current_layer_raises(ctx: &Context) -> Option<Vec<egui::LayerId>> {
        let stamp = (ctx.viewport_id(), ctx.cumulative_frame_nr(), ctx.cumulative_pass_nr());
        let key = layer_raise_key(ctx);
        let raises = ctx.data(|data| data.get_temp::<LayerRaises>(key));
        match raises {
            Some(raises) if raises.stamp == stamp => (!raises.exhausted).then_some(raises.layers),
            _ => Some(Vec::new()),
        }
    }

    /// Current shared show intent supplements egui's previous-frame modal state.
    pub fn modal_input_blocked(ctx: &Context) -> bool {
        let stamp = (ctx.cumulative_frame_nr(), ctx.cumulative_pass_nr());
        let key = modal_boundary_key(ctx);
        ctx.data(|data| data.get_temp::<ModalBoundary>(key).is_some_and(|boundary| boundary.stamp == stamp))
            || ctx.memory(|memory| memory.top_modal_layer().is_some())
    }

    pub fn reserve_host_escape(ctx: &Context) -> bool {
        if Self::modal_input_blocked(ctx) {
            return false;
        }
        let event = ctx.input_mut(|input| {
            let event = input
                .events
                .iter()
                .find(|event| {
                    matches!(event,
                        egui::Event::Key { key: egui::Key::Escape, pressed: true, modifiers, .. }
                            if modifiers.matches_logically(egui::Modifiers::NONE)
                    )
                })
                .cloned();
            if event.is_some() {
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
            }
            event
        });
        if let Some(event) = event {
            let stamp = (ctx.cumulative_frame_nr(), ctx.cumulative_pass_nr());
            let key = host_escape_key(ctx);
            ctx.data_mut(|data| data.insert_temp(key, HostEscape { stamp, event }));
            true
        } else {
            false
        }
    }

    /// A panel on `edge`, keyed by `salt` within the Ui it is shown in.
    pub fn edge(self, salt: impl egui::AsIdSalt, edge: Edge) -> egui::Panel {
        let panel = match edge {
            Edge::Left => egui::Panel::left(salt),
            Edge::Right => egui::Panel::right(salt),
            Edge::Top => egui::Panel::top(salt),
            Edge::Bottom => egui::Panel::bottom(salt),
        };
        if self.adaptive() && !self.floating() { panel.show_separator_line(false) } else { panel }
    }

    /// Stock Panel owns layout, persistence, interaction and its unmodified semantic Frame.
    /// Only ordinary `show` is exposed; animated hosts need their own clip contract.
    pub fn show_edge<R>(self, ui: &mut egui::Ui, panel: egui::Panel, frame: Frame, body: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
        let background = self.background(ui.ctx(), frame);
        Self::show_edge_background(ui, panel, frame, background, body)
    }

    pub fn show_edge_background<R>(
        ui: &mut egui::Ui,
        panel: egui::Panel,
        frame: Frame,
        background: Option<Background>,
        body: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::InnerResponse<R> {
        let Some(background) = background else { return panel.frame(frame).show(ui, body) };
        let slot = ui.painter().add(egui::Shape::Noop);
        let mut host = None;
        let shown = panel.frame(frame).show(ui, |ui| {
            // Capture before the body can narrow the clip or change opacity. Frame adds
            // no content clip, so this is the host/slide clip including its padding.
            let painter = ui.painter().clone();
            let inner = body(ui);
            host = Some((painter, ui.min_rect()));
            inner
        });
        if let Some((painter, content)) = host
            && painter.is_visible()
            && painter.clip_rect().intersects(frame.widget_rect(content))
        {
            // min_rect is the actual Frame content, even if Panel clamps its response.
            painter.set(slot, background(&painter, frame, content, egui::Shape::Noop));
        }
        shown
    }

    pub fn window<'a>(self, title: impl Into<WidgetText>) -> egui::Window<'a> {
        egui::Window::new(title).enabled(self.input_enabled())
    }

    /// Anchored editors inherit their containing widget's local style. Their host stays a
    /// popup in both layouts; only the universal module constructs the egui popup frame.
    pub fn popup_frame(style: &egui::Style) -> Frame {
        let mut frame = Frame::popup(style);
        frame.stroke = Tokens::get().borders.stroke(solidcraft_theme::BorderRole::Primary, frame.stroke);
        frame
    }

    pub fn menu_frame() -> Frame {
        let t = Tokens::get();
        egui::Frame::NONE
            .fill(t.dialog_body)
            .stroke(t.primary_border(t.dialog_border))
            .corner_radius(t.radius_xl)
            .inner_margin(egui::Margin::symmetric(t.space_6 as i8, t.space_6 as i8))
            .shadow(egui::Shadow {
                offset: [0, t.space_14 as i8],
                blur: (t.space_24 + t.space_12) as u8,
                spread: 0,
                color: t.app_bar.gamma_multiply(0.18),
            })
    }

    pub fn submenu(self) -> egui::menu::SubMenu {
        egui::menu::SubMenu::new()
    }

    pub fn menu<'a>(self, response: &egui::Response) -> BackingPopup<'a> {
        BackingPopup(egui::Popup::menu(response))
    }

    pub fn context_menu<'a>(self, response: &egui::Response) -> BackingPopup<'a> {
        BackingPopup(egui::Popup::context_menu(response))
    }

    pub fn tooltip_frame(self, ctx: &Context) -> Frame {
        let t = Tokens::get();
        self.frame(ctx, t.dialog_body, crate::frame::Side::Right)
            .stroke(t.primary_border(t.dialog_border))
            .inner_margin(egui::Margin::symmetric(t.space_8 as i8, t.space_4 as i8))
    }

    pub fn modal(self, id: Id) -> ModalHost {
        ModalHost { modal: BackingModal { modal: egui::Modal::new(id).area(egui::Modal::default_area(id).fade_in(false)), id } }
    }

    pub fn central(self) -> egui::CentralPanel {
        egui::CentralPanel::default()
    }

    fn input_enabled(self) -> bool {
        self.surface == Surface::InputShield || !input_blocked()
    }
}

#[cfg(test)]
#[path = "container_tests.rs"]
mod tests;
