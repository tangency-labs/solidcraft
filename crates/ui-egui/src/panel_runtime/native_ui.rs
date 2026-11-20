use solidcraft_engine::panels::*;

use super::drag_paint::WindowCard;
use super::workspace::{WindowDragEvent, WindowMenuEntry, WorkspacePanelHost};
use super::{ContentServices, ContentSlot, FramePanelContent, PanelContext, PanelController, PanelHost, PanelPresentation, PanelRuntime};
use crate::ui_kit::{self, BackingTooltipExt as _, Container, Edge, HostChrome, Surface};
use crate::window_chrome::TitleBar;
use solidcraft_theme::Tokens;

pub(super) enum WindowAction {
    Activate(InstanceId),
    Expand(SectionId),
    Content(InstanceId, ControlId),
    Redock,
    /// The grip is dragging the active panel back towards the app.
    Drag(WindowDragEvent),
    /// An entry of the window's menu: the panel moves back into the app.
    Layout(LayoutAction),
}

/// A panel window in the app's own look, in place of the system's: the app's
/// title bar, and how the app lays out its panel cards (the host's metrics and
/// the shell's gutter between the app window's edge and its cards).
#[derive(Clone, Copy, Debug)]
pub(super) struct CardWindow {
    /// Which of the OS's window controls share the title bar.
    pub title_bar: TitleBar,
    pub metrics: super::layout::HostMetrics,
    pub gutter: f32,
}

impl CardWindow {
    pub(super) fn of(title_bar: TitleBar, frame: Option<&super::workspace::HostFrame>) -> Self {
        frame.map_or_else(
            || Self { title_bar, metrics: super::layout::HostMetrics::default(), gutter: Tokens::get().space_8 },
            |frame| Self { title_bar, metrics: frame.layout.metrics, gutter: frame.layout.capacity.origin.x.max(0.0) },
        )
    }
}

/// With `card`, the window has the app's own look: the app's title bar, whose
/// grip drags the panel back into the app and whose menu docks it there, and
/// the layout of the panel's docked card. The app's host (`root`) gives what the
/// docked header has: the menu's entries, the panel's own content in the
/// title's place (`ContentSlot::Title`) and the header actions it hides.
/// Without `card`, it is the system's window: its title bar, and the earlier
/// layout.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint(
    runtime: &mut PanelRuntime,
    services: &mut ContentServices<'_>,
    content: &mut dyn FramePanelContent,
    host: &HostId,
    section: &Section,
    ui: &mut egui::Ui,
    enabled: bool,
    root: Option<&WorkspacePanelHost>,
    card: Option<CardWindow>,
) -> PanelResult<Option<WindowAction>> {
    if ui.input(|input| input.viewport().close_requested()) {
        return Ok(Some(WindowAction::Redock));
    }
    if ui.input(|input| input.viewport().visible() == Some(false)) {
        runtime.mount_native(host, &ResolvedLayout::default(), &mut super::workspace::EmptyPanelHost, services)?;
        return Ok(None);
    }
    let outer = super::workspace::logical_rect(ui.max_rect());
    let mut action = None;
    let availability = section.tabs.iter().map(|id| (id.clone(), runtime.layout().availability(id))).collect();
    let active = presented_tab(section, &availability);
    let mut title_slot = None;
    if let Some(card) = card {
        if let Some(active) = active {
            let title = runtime.instance_presentation(active)?.title;
            let rtl = runtime.environment().direction == Direction::Rtl;
            // As in the docked header, the panel's own content may take the title's place.
            let title_content = root.is_some_and(|root| root.has_header_content(active));
            let menu = || root.map_or_else(Vec::new, |root| root.window_menu(runtime, active));
            let (bar_action, caption, slot) = window_bar(ui, card, host, active, &title, rtl, enabled, title_content, &menu);
            action = bar_action;
            title_slot = slot;
            if let Some(caption) = caption {
                caption.send(ui.ctx());
            }
        }
        if card.title_bar.caption_buttons {
            crate::window_chrome::resize_zones(ui);
        }
    }
    // As the panel's docked card (the default), so a dragged-out window keeps its
    // look, or with the earlier layout.
    let as_card = card.is_some();
    let panel = Container::new(ui.ctx(), Surface::Viewport).central();
    let frame = Container::new(ui.ctx(), Surface::Panel).edge_frame(solidcraft_theme::Tokens::get().dialog_body);
    Container::show_central(ui, panel, frame, |ui| {
        ui.add_enabled_ui(enabled, |ui| {
            let Some(active) = active else {
                return runtime.mount_native(host, &ResolvedLayout::default(), &mut super::workspace::EmptyPanelHost, services);
            };
            let presentation = runtime.instance_presentation(active)?;
            let layout = runtime.layout();
            let spec = layout.registry().get(&layout.layout().instance(active)?.kind)?;
            let environment = runtime.environment();
            let tabs = section.tabs.iter().filter(|id| availability.get(*id) == Some(&Availability::Available));
            let rtl = environment.direction == Direction::Rtl;
            // As the panel's card, the window leaves out the actions its docked header hides.
            let hidden = |action: &ControlId| as_card && root.is_some_and(|root| root.header_action_hidden(active, action));
            let declared: Vec<_> = spec
                .header_actions
                .iter()
                .filter(|declared| !hidden(&declared.id))
                .filter_map(|declared| Some((declared, presentation.actions.iter().find(|entry| entry.id == declared.id)?)))
                .collect();
            if section.tabs.len() > 1 || section.collapsed || (!as_card && !presentation.actions.is_empty()) {
                ui.horizontal_wrapped(|ui| {
                    if section.tabs.len() > 1 {
                        for id in tabs {
                            if let Ok(tab) = runtime.instance_presentation(id)
                                && ui_kit::choice_chip(ui, &id.0, &tab.title, id == active).clicked()
                            {
                                action = Some(WindowAction::Activate(id.clone()));
                            }
                        }
                    }
                    let size = egui::Vec2::splat(Tokens::get().form_close_size);
                    if section.collapsed {
                        let (id, bounds) = ui.allocate_space(size);
                        let label = solidcraft_ui_text::tr("ui.panel_host.expand");
                        let response = ui_kit::panel_host_disclosure(ui, id, bounds, label, true, true, environment.motion == super::Motion::Reduced);
                        if response.clicked() {
                            action = Some(WindowAction::Expand(section.id.clone()));
                        }
                    }
                    if !as_card {
                        for (declared, entry) in &declared {
                            let (_, bounds) = ui.allocate_space(size);
                            if action_control(ui, active, declared, entry, bounds, rtl) {
                                action = Some(WindowAction::Content(active.clone(), entry.id.clone()));
                            }
                        }
                    }
                });
            }
            let area = ui.available_rect_before_wrap();
            let mut body = area;
            if let Some(metrics) = card.map(|card| card.metrics) {
                // Rows and body are placed as the card places them, with no spacing between.
                let mut top = area.top();
                if !section.collapsed && !declared.is_empty() {
                    // The panel's actions sit as in its docked card's action row: control
                    // squares from the trailing edge, inset as the header's controls are.
                    let row = egui::Rect::from_min_size(area.min, egui::vec2(area.width(), metrics.control_extent.min(area.height()).max(0.0)));
                    top = row.bottom();
                    for ((declared, entry), bounds) in declared.iter().zip(card_action_row(row, declared.len(), metrics, rtl)) {
                        if action_control(ui, active, declared, entry, bounds, rtl) {
                            action = Some(WindowAction::Content(active.clone(), entry.id.clone()));
                        }
                    }
                }
                // As in the docked card: the body ends above the footer's reserve.
                body = egui::Rect::from_min_max(
                    egui::pos2(area.left(), top + metrics.content_header_height),
                    egui::pos2(area.right(), area.bottom() - metrics.footer_height),
                );
            }
            let (min, max) = tab_bounds(runtime.layout().layout(), runtime.layout().registry(), section)?;
            if !outer.size.within(min, max) || !body.is_positive() {
                return runtime.mount_native(host, &ResolvedLayout::default(), &mut super::workspace::EmptyPanelHost, services);
            }
            let geometry = ResolvedPanel {
                instance: active.clone(),
                section: section.id.clone(),
                zone: Zone::Window,
                outer,
                body: (!section.collapsed).then(|| super::workspace::logical_rect(body)),
                painted: vec![outer],
                input: Vec::new(),
            };
            runtime.mount_native(
                host,
                &ResolvedLayout { panels: vec![geometry], ..Default::default() },
                &mut NativeContent { ui, content, inset: as_card, title: title_slot },
                services,
            )
        })
        .inner
    })
    .inner?;
    Ok(action)
}

/// A panel window's height besides its body's content. In the app's own look
/// (`card`): its title bar, the action row when the panel has actions, the
/// content header and footer. Else the row of its actions, when the panel has
/// any: the system draws the title bar outside the window's content.
pub(super) fn window_chrome(ctx: &egui::Context, card: Option<CardWindow>, actions: bool) -> f32 {
    match card {
        Some(CardWindow { metrics, .. }) => {
            let row = if actions { metrics.control_extent } else { 0.0 };
            crate::toolbar::APP_BAR_HEIGHT + row + metrics.content_header_height + metrics.footer_height
        }
        None if actions => Tokens::get().form_close_size + ctx.global_style().spacing.item_spacing.y,
        None => 0.0,
    }
}

/// What a dragged-out panel's window has in place of its card's header, for the
/// card's body to keep its size and place. In the app's own look (`card`): the
/// title bar. Else the row of the panel's actions; none for a side card
/// (`side_card`), which is painted with its actions in it.
pub(super) fn drop_chrome(ctx: &egui::Context, card: Option<CardWindow>, actions: bool, side_card: bool) -> f32 {
    match card {
        Some(_) => crate::toolbar::APP_BAR_HEIGHT,
        None if side_card => 0.0,
        None => window_chrome(ctx, None, actions),
    }
}

/// One of the panel's actions in its window; true when clicked.
fn action_control(
    ui: &mut egui::Ui,
    active: &InstanceId,
    declared: &HeaderActionSpec,
    entry: &super::RuntimeAction,
    bounds: egui::Rect,
    rtl: bool,
) -> bool {
    let id = ui.make_persistent_id((active, &entry.id));
    let chrome = HostChrome {
        label: &entry.label,
        icon: Some(&declared.icon),
        icon_only: true,
        attention: false,
        selected: entry.pressed.unwrap_or(false),
        enabled: entry.enabled,
        drag: false,
        title: false,
        grip_only: false,
        rtl,
    };
    let response = ui_kit::panel_host_control(ui, id, bounds, chrome);
    if let Some(pressed) = entry.pressed {
        ui.ctx().accesskit_node_builder(response.id, |node| {
            node.set_toggled(if pressed { egui::accesskit::Toggled::True } else { egui::accesskit::Toggled::False });
        });
    }
    if let Some(reason) = &entry.disabled_reason {
        response.clone().backing_on_hover_text(reason);
    }
    response.clicked()
}

/// Where `count` actions sit in a card's action row `row`, in their declared
/// order: control squares from the trailing edge, inset as the header's
/// controls are, mirrored right to left.
pub(super) fn card_action_row(row: egui::Rect, count: usize, metrics: super::layout::HostMetrics, rtl: bool) -> Vec<egui::Rect> {
    if count == 0 {
        return Vec::new();
    }
    let margin = ((metrics.header_height - metrics.control_extent) / 2.0).max(0.0).min(row.width() / 2.0);
    let extent = metrics.control_extent.min(row.height()).min((row.width() - margin) / count as f32).max(0.0);
    let mut cursor = if rtl { row.left() + margin } else { row.right() - margin };
    let mut bounds: Vec<_> = (0..count)
        .map(|_| {
            let x = if rtl { cursor } else { cursor - extent };
            cursor = if rtl { cursor + extent } else { cursor - extent };
            egui::Rect::from_min_size(egui::pos2(x, row.center().y - extent / 2.0), egui::Vec2::splat(extent))
        })
        .collect();
    bounds.reverse();
    bounds
}

/// The window a drag shows before the panel opens in it: the panel's body as
/// painted this frame, where its content will be. In the app's own look
/// (`look`) that is under the same title bar, menu button included; else under
/// the window's `chrome`.
pub(super) fn paint_drop(
    ui: &mut egui::Ui,
    card: Option<&WindowCard>,
    chrome: f32,
    look: Option<CardWindow>,
    active: &InstanceId,
    title: &str,
    rtl: bool,
) {
    if let Some(look) = look {
        // The pointer passes through this window: its menu never opens.
        window_bar(ui, look, "drop", active, title, rtl, true, false, &Vec::new);
    }
    // The title bar is an edge of the window, outside the content below it.
    let chrome = if look.is_some() { 0.0 } else { chrome };
    let panel = Container::new(ui.ctx(), Surface::Viewport).central();
    let frame = Container::new(ui.ctx(), Surface::Panel).edge_frame(Tokens::get().dialog_body);
    Container::show_central(ui, panel, frame, |ui| {
        if let Some(card) = card {
            let mut body = ui.max_rect();
            body.min.y = (body.min.y + chrome).min(body.max.y);
            card.paint(ui, body);
        }
    });
}

/// The window's own title bar, as the app window has: the OS's window controls,
/// the panel's title with its grip, which drags the panel back over the app, and
/// at the trailing end the menu (`menu`) of places in the app the panel can dock.
/// With `title_content`, only the grip is drawn, and the title's place is
/// returned for the panel's own content (`ContentSlot::Title`).
#[allow(clippy::too_many_arguments)]
fn window_bar(
    ui: &mut egui::Ui,
    card: CardWindow,
    id: impl std::hash::Hash + std::fmt::Debug,
    active: &InstanceId,
    title: &str,
    rtl: bool,
    enabled: bool,
    title_content: bool,
    menu: &dyn Fn() -> Vec<Vec<WindowMenuEntry>>,
) -> (Option<WindowAction>, Option<crate::window_chrome::WindowAction>, Option<egui::Rect>) {
    let edge = Container::new(ui.ctx(), Surface::ApplicationBar).edge(("sc_panel_window_bar", id), Edge::Top);
    let frame = Container::new(ui.ctx(), Surface::ApplicationBar).edge_frame(Tokens::get().app_bar);
    let height = crate::toolbar::APP_BAR_HEIGHT;
    Container::new(ui.ctx(), Surface::ApplicationBar)
        .show_edge(ui, edge.exact_size(height).show_separator_line(false), frame, |ui| {
            if !enabled {
                ui.disable();
            }
            let mut action = None;
            let mut title_slot = None;
            let rect = ui.max_rect();
            let caption = card.title_bar.show(ui, rect, |ui, content| {
                // As far from the window's edge as the docked card's menu button is
                // from the app window's edge: the shell's gutter, then the header's
                // own margin.
                let metrics = card.metrics;
                let extent = metrics.control_extent.min(content.height()).min(content.width());
                let margin = (card.gutter + ((metrics.header_height - extent) / 2.0).max(0.0)).min((content.width() - extent).max(0.0));
                let x = if rtl { content.left() + margin } else { content.right() - margin - extent };
                let bounds = egui::Rect::from_min_size(egui::pos2(x, content.center().y - extent / 2.0), egui::Vec2::splat(extent));
                let title_area = if rtl { content.with_min_x(bounds.right()) } else { content.with_max_x(bounds.left()) };
                action = menu_button(ui, bounds, active, rtl, menu).map(WindowAction::Layout);
                // The grip that stands in for the title before the panel's own content, as wide
                // as in the docked header.
                let grip = title_content.then(|| metrics.control_extent.min(Tokens::get().space_10 + Tokens::get().space_8));
                let (drag, slot) = title_grip(ui, title_area, active, title, rtl, grip);
                title_slot = slot;
                if let Some(drag) = drag {
                    action = Some(WindowAction::Drag(drag));
                }
            });
            (action, caption, title_slot)
        })
        .inner
}

/// The window's menu button and its menu: the places in the app the panel can
/// dock, and Close, as the docked header's menu offers them.
fn menu_button(
    ui: &mut egui::Ui,
    bounds: egui::Rect,
    active: &InstanceId,
    rtl: bool,
    menu: &dyn Fn() -> Vec<Vec<WindowMenuEntry>>,
) -> Option<LayoutAction> {
    let label = solidcraft_ui_text::tr("ui.panel_host.menu");
    let id = ui.make_persistent_id(("panel-window-menu", active));
    let chrome = HostChrome {
        label,
        icon: Some("dots"),
        icon_only: true,
        attention: false,
        selected: false,
        enabled: true,
        drag: false,
        title: false,
        grip_only: false,
        rtl,
    };
    let response = ui_kit::panel_host_control(ui, id, bounds, chrome);
    let mut chosen = None;
    Container::new(ui.ctx(), Surface::Popup).menu(&response).frame(ui_kit::Menu::frame()).show(|ui| {
        ui.set_min_width(ui_kit::LIST_W.min(ui.ctx().content_rect().width() - 2.0 * Tokens::get().space_12).max(0.0));
        ui.spacing_mut().item_spacing.y = 0.0;
        for (index, group) in menu().into_iter().enumerate() {
            if index > 0 {
                ui_kit::Menu::separator(ui);
            }
            for (row, entry) in group.into_iter().enumerate() {
                // By position: two panels to join can share a title.
                let registry_id = format!("panel-window-menu:{index}:{row}");
                if ui_kit::MenuItem::new(&registry_id, &entry.label, entry.icon).show(ui).clicked() {
                    chosen = Some(entry.action);
                }
            }
        }
    });
    chosen
}

/// The panel's title with the section grip a docked header has, here dragging
/// the panel back over the app. Positions are screen points; Escape cancels.
/// With `grip_only` (the grip's width), the title is left out, and what is left
/// of `content` after the grip is returned.
fn title_grip(
    ui: &mut egui::Ui,
    content: egui::Rect,
    active: &InstanceId,
    title: &str,
    rtl: bool,
    grip_only: Option<f32>,
) -> (Option<WindowDragEvent>, Option<egui::Rect>) {
    let t = Tokens::get();
    let width = match grip_only {
        Some(grip) => grip,
        None => {
            let text = ui.painter().layout_no_wrap(title.to_owned(), ui_kit::type_face::semibold(t.font_heading), t.form_ink).size().x;
            // As the kit lays out a title control: insets, the grip and its gap, the text.
            2.0 * t.space_12 + t.space_10 + t.space_8 + text
        }
    }
    .min(content.width());
    let x = if rtl { content.right() - width } else { content.left() };
    let bounds = egui::Rect::from_min_size(egui::pos2(x, content.top()), egui::vec2(width, content.height()));
    let rest = grip_only.map(|_| if rtl { content.with_max_x(bounds.left()) } else { content.with_min_x(bounds.right()) });
    let id = ui.make_persistent_id(("panel-window-title", active));
    let chrome = HostChrome {
        label: title,
        icon: None,
        icon_only: false,
        attention: false,
        selected: false,
        enabled: true,
        drag: true,
        title: true,
        grip_only: grip_only.is_some(),
        rtl,
    };
    let response = ui_kit::panel_host_control(ui, id, bounds, chrome);
    response.clone().backing_on_hover_text(solidcraft_ui_text::tr("ui.panel_host.dock_grip"));
    let at = ui.input(|input| {
        let pointer = input.pointer.latest_pos()?;
        let origin = input.viewport().inner_rect?.min;
        Some(Point { x: origin.x + pointer.x, y: origin.y + pointer.y }).filter(|at| at.validate().is_ok())
    });
    let drag = if response.drag_started() {
        at.map(|at| WindowDragEvent::Begin { instance: active.clone(), at })
    } else if response.drag_stopped() {
        Some(at.map_or(WindowDragEvent::Cancel, |at| WindowDragEvent::Release { at }))
    } else if response.dragged() {
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) { Some(WindowDragEvent::Cancel) } else { at.map(|at| WindowDragEvent::Move { at }) }
    } else {
        None
    };
    (drag, rest)
}

struct NativeContent<'a> {
    ui: &'a mut egui::Ui,
    content: &'a mut dyn FramePanelContent,
    /// The content keeps the docked card's inset.
    inset: bool,
    /// The title's place in the window's title bar, for the panel's own content there.
    title: Option<egui::Rect>,
}

impl PanelHost for NativeContent<'_> {
    fn mount(
        &mut self,
        geometry: &ResolvedPanel,
        _presentation: &PanelPresentation,
        context: &PanelContext,
        controller: &mut dyn PanelController,
        services: &mut ContentServices<'_>,
    ) -> PanelResult<()> {
        if let Some(title) = self.title {
            // Before the body, as in the docked header: the body then leaves out what
            // the title's place holds. The title bar is outside this content's area.
            let mut bar = self.ui.new_child(egui::UiBuilder::new().id_salt(("panel-window-title-content", &geometry.instance)).max_rect(title));
            bar.set_clip_rect(title);
            super::workspace::render_window_title(&mut bar, title, context, controller, services, self.content)?;
        }
        if let Some(body) = geometry.body {
            let rect = super::workspace::egui_rect(body);
            let rect = if self.inset { super::workspace::content_inset(rect) } else { rect };
            let mut ui = self.ui.new_child(egui::UiBuilder::new().id_salt(&geometry.instance).max_rect(rect));
            ui.set_clip_rect(ui.clip_rect().intersect(rect));
            self.content.render(ContentSlot::Body, &mut ui, context, controller, services)?;
        }
        Ok(())
    }
}
