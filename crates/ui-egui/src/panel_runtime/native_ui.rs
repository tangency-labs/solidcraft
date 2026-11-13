use solidcraft_engine::panels::*;

use super::{ContentServices, ContentSlot, FramePanelContent, PanelContext, PanelController, PanelHost, PanelPresentation, PanelRuntime};
use crate::ui_kit::{self, BackingTooltipExt as _, Container, HostChrome, Surface};
use solidcraft_theme::Tokens;

pub(super) enum WindowAction {
    Activate(InstanceId),
    Expand(SectionId),
    Content(InstanceId, ControlId),
    Redock,
}

pub(super) fn paint(
    runtime: &mut PanelRuntime,
    services: &mut ContentServices<'_>,
    content: &mut dyn FramePanelContent,
    host: &HostId,
    section: &Section,
    ui: &mut egui::Ui,
    enabled: bool,
) -> PanelResult<Option<WindowAction>> {
    if ui.input(|input| input.viewport().close_requested()) {
        return Ok(Some(WindowAction::Redock));
    }
    if ui.input(|input| input.viewport().visible() == Some(false)) {
        runtime.mount_native(host, &ResolvedLayout::default(), &mut super::workspace::EmptyPanelHost, services)?;
        return Ok(None);
    }
    let outer = super::workspace::logical_rect(ui.max_rect());
    let panel = Container::new(ui.ctx(), Surface::Viewport).central();
    let frame = Container::new(ui.ctx(), Surface::Panel).edge_frame(solidcraft_theme::Tokens::get().dialog_body);
    let mut action = None;
    Container::show_central(ui, panel, frame, |ui| {
        ui.add_enabled_ui(enabled, |ui| {
            let availability = section.tabs.iter().map(|id| (id.clone(), runtime.layout().availability(id))).collect();
            let Some(active) = presented_tab(section, &availability) else {
                return runtime.mount_native(host, &ResolvedLayout::default(), &mut super::workspace::EmptyPanelHost, services);
            };
            let presentation = runtime.instance_presentation(active)?;
            let layout = runtime.layout();
            let spec = layout.registry().get(&layout.layout().instance(active)?.kind)?;
            let environment = runtime.environment();
            let tabs = section.tabs.iter().filter(|id| availability.get(*id) == Some(&Availability::Available));
            if section.tabs.len() > 1 || section.collapsed || !presentation.actions.is_empty() {
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
                    for declared in &spec.header_actions {
                        let Some(entry) = presentation.actions.iter().find(|entry| entry.id == declared.id) else { continue };
                        let (_, bounds) = ui.allocate_space(size);
                        let id = ui.make_persistent_id((active, &entry.id));
                        let response = ui_kit::panel_host_control(
                            ui,
                            id,
                            bounds,
                            HostChrome {
                                label: &entry.label,
                                icon: Some(&declared.icon),
                                icon_only: true,
                                attention: false,
                                selected: entry.pressed.unwrap_or(false),
                                enabled: entry.enabled,
                                drag: false,
                                title: false,
                                grip_only: false,
                                rtl: environment.direction == Direction::Rtl,
                            },
                        );
                        if let Some(pressed) = entry.pressed {
                            ui.ctx().accesskit_node_builder(response.id, |node| {
                                node.set_toggled(if pressed { egui::accesskit::Toggled::True } else { egui::accesskit::Toggled::False });
                            });
                        }
                        if let Some(reason) = &entry.disabled_reason {
                            response.clone().backing_on_hover_text(reason);
                        }
                        if response.clicked() {
                            action = Some(WindowAction::Content(active.clone(), entry.id.clone()));
                        }
                    }
                });
            }
            let body = ui.available_rect_before_wrap();
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
            runtime.mount_native(host, &ResolvedLayout { panels: vec![geometry], ..Default::default() }, &mut NativeContent { ui, content }, services)
        })
        .inner
    })
    .inner?;
    Ok(action)
}

struct NativeContent<'a> {
    ui: &'a mut egui::Ui,
    content: &'a mut dyn FramePanelContent,
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
        if let Some(body) = geometry.body {
            let rect = super::workspace::egui_rect(body);
            let mut ui = self.ui.new_child(egui::UiBuilder::new().id_salt(&geometry.instance).max_rect(rect));
            ui.set_clip_rect(ui.clip_rect().intersect(rect));
            self.content.render(ContentSlot::Body, &mut ui, context, controller, services)?;
        }
        Ok(())
    }
}
