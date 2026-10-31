//! The only constructors for outer UI hosts. Content widgets belong to the other kit modules.
//! A surface declares its purpose; the shell supplies the layout once per frame. Never infer
//! floating/docked presentation from a panel name or keep a second layout preference here.
use crate::frame::Side;
use crate::{shell::LayoutMode, theme::Tokens};
use egui::{Color32, Context, Frame, Id, Stroke, WidgetText};

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
    pub(crate) fn tooltip(response: &egui::Response, text: &str) {
        let ctx = &response.ctx;
        let mut tip = egui::Tooltip::for_enabled(response).at_pointer();
        tip.popup = tip.popup.frame(Self::new(ctx, Surface::Toast).tooltip_frame(ctx));
        tip.show(|ui| {
            ui.label(text);
            crate::scenario::publish_rect("kit:tooltip", ui.min_rect());
        });
    }

    /// [`Self::tooltip`] holding what `add` draws (a picture, say), beside `response` rather than
    /// at the pointer, so it does not cover the row's neighbours.
    pub(crate) fn tooltip_ui(response: &egui::Response, add: impl FnOnce(&mut egui::Ui)) {
        let ctx = &response.ctx;
        let mut tip = egui::Tooltip::for_enabled(response);
        tip.popup = tip.popup.align(egui::RectAlign::RIGHT_START).frame(Self::new(ctx, Surface::Toast).tooltip_frame(ctx));
        tip.show(|ui| {
            add(ui);
            crate::scenario::publish_rect("kit:tooltip", ui.min_rect());
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
        painter.add(frame.paint(content));
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

    pub fn menu<'a>(self, response: &egui::Response) -> egui::Popup<'a> {
        egui::Popup::menu(response)
    }

    pub fn context_menu<'a>(self, response: &egui::Response) -> egui::Popup<'a> {
        egui::Popup::context_menu(response)
    }

    pub(crate) fn tooltip_frame(self, ctx: &Context) -> Frame {
        let t = Tokens::get();
        self.frame(ctx, t.dialog_body, crate::frame::Side::Right)
            .stroke(t.primary_border(t.dialog_border))
            .inner_margin(egui::Margin::symmetric(t.space_8 as i8, t.space_4 as i8))
    }

    pub fn modal(self, id: Id) -> egui::Modal {
        egui::Modal::new(id).area(egui::Modal::default_area(id).fade_in(false))
    }

    pub fn central(self) -> egui::CentralPanel {
        egui::CentralPanel::default()
    }

    fn input_enabled(self) -> bool {
        self.surface == Surface::InputShield || !crate::issue_report::cad_input_blocked()
    }
}
