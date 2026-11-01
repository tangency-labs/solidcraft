//! The only constructors for outer UI hosts. Content widgets belong to the other kit modules.
//! A surface declares its purpose; the shell supplies the layout once per frame. Never infer
//! floating/docked presentation from a panel name or keep a second layout preference here.
use crate::frame::Side;
use crate::{shell::LayoutMode, theme::Tokens};
use egui::{Color32, Context, Frame, Id, Stroke, WidgetText};

pub(super) type Background = fn(&egui::Painter, Frame, egui::Rect, egui::Shape) -> egui::Shape;

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

/// The feature sheet's deliberately restricted stock Window (no title, scrolling or fades).
/// Keep its builder private: the marker's layer and host clip are owned here.
pub(crate) struct FeatureWindow<'a> {
    window: egui::Window<'a>,
    id: Id,
    frame: Frame,
}

impl<'a> FeatureWindow<'a> {
    pub(super) fn new(window: egui::Window<'a>, id: Id, frame: Frame) -> Self {
        Self { window, id, frame }
    }

    pub(crate) fn fixed_size(mut self, size: egui::Vec2) -> Self {
        self.window = self.window.fixed_size(size);
        self
    }

    /// Borrow a caller's open flag only for this showing, retaining stock close handling.
    pub(crate) fn open<'open>(self, open: &'open mut bool) -> FeatureWindow<'open>
    where
        'a: 'open,
    {
        let window: egui::Window<'open> = self.window;
        FeatureWindow { window: window.open(open), id: self.id, frame: self.frame }
    }

    pub(crate) fn show<R>(self, ctx: &Context, body: impl FnOnce(&mut egui::Ui) -> R) -> Option<egui::InnerResponse<Option<R>>> {
        let background = Container::floating_card(Surface::Feature).background(ctx, self.frame);
        self.show_background(ctx, background, body)
    }

    pub(super) fn show_background<R>(
        self,
        ctx: &Context,
        background: Option<Background>,
        body: impl FnOnce(&mut egui::Ui) -> R,
    ) -> Option<egui::InnerResponse<Option<R>>> {
        let Some(background) = background else { return self.window.show(ctx, body) };
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, self.id));
        let slot = painter.add(egui::Shape::Noop);
        let mut host_painter = None;
        let shown = self.window.show(ctx, |ui| {
            // Resize's content clip omits the frame border/padding. This bounded Window
            // uses Area's default viewport clip, with no fades or native title bar.
            let mut painter = ui.painter().clone();
            painter.set_clip_rect(ctx.content_rect());
            host_painter = Some(painter);
            body(ui)
        });
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

thread_local! {
    /// The areas built as [`Surface::Canvas`]: a value box, a dimension's editor, a readout.
    static CANVAS_AREAS: std::cell::RefCell<std::collections::HashSet<Id>> = std::cell::RefCell::default();
}

/// Is `layer` a model-anchored editor or readout ([`Surface::Canvas`])? Such an area sits on the
/// model and scrolls nothing, so the model's wheel and trackpad navigation go on under it (#10061).
pub(crate) fn is_canvas(layer: egui::LayerId) -> bool {
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

    pub(crate) fn for_layout(layout: LayoutMode, surface: Surface) -> Self {
        Self { surface, layout, joined: None }
    }

    /// Floating workspace cards retain their shared chrome even in compact ribbon layouts.
    pub(crate) fn floating_card(surface: Surface) -> Self {
        Self::for_layout(LayoutMode::Floating, surface)
    }

    /// Shared bottom-strip placement, used by History and other workspace strips.
    pub(crate) fn bottom_strip_rect(self, available: egui::Rect, height: f32) -> egui::Rect {
        let inset = available.shrink(self.gap());
        egui::Rect::from_min_max(egui::pos2(inset.left(), (inset.bottom() - height).max(inset.top())), inset.max)
    }

    /// Anchored navigation surfaces share the compass/nav-bar frame and token padding.
    pub(crate) fn navigation_card<R>(
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
    pub(crate) fn below_navigation_row(bounds: egui::Rect) -> egui::Rect {
        let t = Tokens::get();
        egui::Rect::from_min_max(egui::pos2(bounds.left(), (bounds.top() + t.control_height + t.space_8 * 2.0).min(bounds.bottom())), bounds.max)
    }

    /// Inline navigation band using the same chrome and padding as anchored HUDs.
    pub(crate) fn navigation_contents<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
        let container = Self::new(ui.ctx(), Surface::Navigation);
        let frame = container.frame(ui.ctx(), Tokens::get().overlay, Side::Left).inner_margin(Tokens::get().space_4);
        container.show_frame(ui, frame, ui.max_rect(), |ui| super::scope(ui, add))
    }

    /// Compact selection actions above canvas geometry, using the navigation host policy.
    pub(crate) fn selection_card<R>(
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
    pub(crate) fn show_tooltip<R>(parent: Id, tip: egui::Tooltip<'_>, body: impl FnOnce(&mut egui::Ui) -> R) -> Option<egui::InnerResponse<R>> {
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

    pub(crate) fn tooltip(response: &egui::Response, text: &str) {
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
                crate::scenario::publish_rect("kit:tooltip", ui.min_rect());
            };
            match slot {
                Some(slot) => slot.body(ui, body),
                None => body(ui),
            }
        });
    }

    /// [`Self::tooltip`] holding what `add` draws (a picture, say), beside `response` rather than
    /// at the pointer, so it does not cover the row's neighbours.
    pub(crate) fn tooltip_ui(response: &egui::Response, add: impl FnOnce(&mut egui::Ui)) {
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
                crate::scenario::publish_rect("kit:tooltip", ui.min_rect());
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
        if self.floating() { crate::shell::SHELL_GAP } else { 0.0 }
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
    pub(crate) fn tool_frame(self, mut frame: Frame, right_docked: bool) -> Frame {
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
    pub(crate) fn show_frame<R>(
        self,
        ui: &mut egui::Ui,
        frame: Frame,
        bounds: egui::Rect,
        body: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::InnerResponse<R> {
        let background = self.background(ui.ctx(), frame);
        self.show_frame_background(ui, frame, bounds, background, body)
    }

    /// Stock Frame path for backing cards; no additional inset chrome is introduced.
    pub(crate) fn show_backing<R>(ui: &mut egui::Ui, frame: Frame, body: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
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
    pub(crate) fn set_backing(painter: &egui::Painter, slot: egui::layers::ShapeIdx, frame: Frame, content: egui::Rect) {
        let shape = frame.paint(content);
        painter.set(slot, if crate::blur::enabled() { crate::blur::decorate(painter, frame, content, shape) } else { shape });
    }

    pub(crate) fn show_central<R>(
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
    pub(crate) fn paint_inset(painter: &egui::Painter, widget: egui::Rect, frame: Frame) {
        let t = Tokens::get();
        if let Some(bevel) = t.borders.bevel {
            let mut chrome = painter.clone();
            chrome.set_clip_rect(widget.intersect(painter.ctx().content_rect()));
            bevel.paint(&chrome, widget, frame.corner_radius, false, true);
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
        // A host's content clip excludes its padding. The companion belongs to the whole
        // host, so clip to that known surface and the viewport instead of the content well.
        let mut painter = painter.clone();
        painter.set_clip_rect(widget.intersect(painter.ctx().content_rect()));
        painter.rect_stroke(
            rect,
            egui::CornerRadius { nw: reduce(radius.nw), ne: reduce(radius.ne), sw: reduce(radius.sw), se: reduce(radius.se) },
            inset.stroke,
            egui::StrokeKind::Inside,
        );
    }

    /// Manually painted chrome uses exactly the same outer and inset policy as hosts.
    pub(crate) fn paint_frame(painter: &egui::Painter, frame: Frame, content: egui::Rect) {
        let shape = frame.paint(content);
        painter.add(if crate::blur::enabled() { crate::blur::decorate(painter, frame, content, shape) } else { shape });
        Self::paint_inset(painter, frame.widget_rect(content), frame);
    }

    /// Semantic manually painted workspace surface; HUDs keep `paint_frame`.
    pub(crate) fn paint_background(self, painter: &egui::Painter, frame: Frame, content: egui::Rect) {
        let shape = frame.paint(content);
        painter.add(if crate::blur::enabled() { crate::blur::decorate(painter, frame, content, shape) } else { shape });
    }

    pub(crate) fn paint_panel_frame(painter: &egui::Painter, frame: Frame, content: egui::Rect) {
        Self::floating_card(Surface::Panel).paint_background(painter, frame, content);
        Self::paint_inset(painter, frame.widget_rect(content), frame);
    }

    /// Decorate the content-hugging frame of a popup or feature sheet after its contents.
    pub(crate) fn finish_content(ui: &egui::Ui, frame: Frame) {
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

    pub fn edge(self, id: impl Into<Id>, edge: Edge) -> egui::Panel {
        let panel = match edge {
            Edge::Left => egui::Panel::left(id),
            Edge::Right => egui::Panel::right(id),
            Edge::Top => egui::Panel::top(id),
            Edge::Bottom => egui::Panel::bottom(id),
        };
        if self.adaptive() && !self.floating() { panel.show_separator_line(false) } else { panel }
    }

    /// Stock Panel owns layout, persistence, interaction and its unmodified semantic Frame.
    /// Only ordinary `show` is exposed; animated hosts need their own clip contract.
    pub(crate) fn show_edge<R>(
        self,
        ui: &mut egui::Ui,
        panel: egui::Panel,
        frame: Frame,
        body: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::InnerResponse<R> {
        let background = self.background(ui.ctx(), frame);
        Self::show_edge_background(ui, panel, frame, background, body)
    }

    pub(super) fn show_edge_background<R>(
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

    pub(super) fn window<'a>(self, title: impl Into<WidgetText>) -> egui::Window<'a> {
        egui::Window::new(title).enabled(self.input_enabled())
    }

    /// Anchored editors inherit their containing widget's local style. Their host stays a
    /// popup in both layouts; only the universal module constructs the egui popup frame.
    pub fn popup_frame(style: &egui::Style) -> Frame {
        let mut frame = Frame::popup(style);
        frame.stroke = Tokens::get().borders.stroke(crate::theme::BorderRole::Primary, frame.stroke);
        frame
    }

    pub(crate) fn menu_frame() -> Frame {
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

    pub(crate) fn submenu(self) -> egui::menu::SubMenu {
        egui::menu::SubMenu::new()
    }

    pub fn menu<'a>(self, response: &egui::Response) -> BackingPopup<'a> {
        BackingPopup(egui::Popup::menu(response))
    }

    pub fn context_menu<'a>(self, response: &egui::Response) -> BackingPopup<'a> {
        BackingPopup(egui::Popup::context_menu(response))
    }

    pub(crate) fn tooltip_frame(self, ctx: &Context) -> Frame {
        let t = Tokens::get();
        self.frame(ctx, t.dialog_body, crate::frame::Side::Right)
            .stroke(t.primary_border(t.dialog_border))
            .inner_margin(egui::Margin::symmetric(t.space_8 as i8, t.space_4 as i8))
    }

    pub fn modal(self, id: Id) -> BackingModal {
        BackingModal { modal: egui::Modal::new(id).area(egui::Modal::default_area(id).fade_in(false)), id }
    }

    pub fn central(self) -> egui::CentralPanel {
        egui::CentralPanel::default()
    }

    fn input_enabled(self) -> bool {
        self.surface == Surface::InputShield || !crate::issue_report::cad_input_blocked()
    }
}
