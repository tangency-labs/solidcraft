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
    /// Native application/title bar; its presentation is independent of workspace layout.
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
                Self::compact_frame(fill, t.dialog_border, edges)
            }
            Surface::Panel | Surface::Chrome if !self.floating() => Self::compact_frame(fill, t.border, self.joined.unwrap_or(DockEdges::ALL)),
            Surface::Feature | Surface::Window => {
                flat.stroke(egui::Stroke::new(1.0, t.dialog_border)).corner_radius(t.radius_xl).shadow(t.shell_shadow())
            }
            Surface::Palette => flat.stroke(egui::Stroke::new(1.0, t.border)).corner_radius(t.radius_xl).shadow(t.shell_shadow()),
            Surface::Panel | Surface::Chrome => flat.stroke(egui::Stroke::new(1.0, t.border)).corner_radius(t.radius_lg).shadow(t.shell_shadow()),
            Surface::Navigation => flat.stroke(egui::Stroke::new(1.0, t.border)).corner_radius(t.radius_md),
            Surface::Toast => flat.stroke(egui::Stroke::new(1.0, t.border)).corner_radius(t.radius_md).shadow(t.shell_shadow()),
            Surface::Popup => Self::popup_frame(&ctx.global_style()).fill(fill),
            Surface::ApplicationBar | Surface::Canvas | Surface::Viewport | Surface::InputShield => flat,
        }
    }

    /// Movable tool sheets round all exposed corners, joining only their right dock edge.
    pub(crate) fn tool_frame(self, mut frame: Frame, right_docked: bool) -> Frame {
        if self.surface == Surface::Feature && !self.floating() {
            frame.corner_radius = Self::compact_rounding(DockEdges { right: right_docked, ..Default::default() });
        }
        frame
    }

    /// Surface of an allocated dock or internal pinned band. Floating card decoration is
    /// supplied by `frame`; edge-hosted panes must not acquire outer shadows or card gaps.
    pub fn edge_frame(self, fill: Color32) -> Frame {
        if self.adaptive() && !self.floating() {
            Self::compact_frame(fill, Tokens::get().border, self.joined.unwrap_or(DockEdges::ALL))
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

    fn compact_frame(fill: Color32, border: Color32, edges: DockEdges) -> Frame {
        // Collapse adjoining borders onto the same one-point strip. North/west outlines
        // sit outside the allocation; south/east outlines sit inside it. Logical panel
        // rectangles still touch exactly, with neither gaps nor doubled strokes.
        Frame::NONE
            .fill(fill)
            .stroke(Stroke::new(1.0, border))
            .outer_margin(egui::Margin { left: -1, top: -1, right: 0, bottom: 0 })
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
        egui::InnerResponse { inner, response: prepared.end(ui) }
    }

    pub fn area(self, id: Id) -> egui::Area {
        egui::Area::new(id).enabled(self.input_enabled())
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
        Frame::popup(style)
    }

    pub(crate) fn menu_frame() -> Frame {
        let t = Tokens::get();
        egui::Frame::NONE
            .fill(t.dialog_body)
            .stroke(Stroke::new(1.0, t.dialog_border))
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
            .stroke(egui::Stroke::new(1.0, t.dialog_border))
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
