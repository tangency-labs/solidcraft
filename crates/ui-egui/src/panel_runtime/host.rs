use crate::ui_kit::BackingTooltipExt as _;
use solidcraft_engine::panels::*;

/// Content receives this value separately from state/model/command services. All
/// geometry is already in logical units; do not multiply it by egui UI scale.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelContext {
    pub instance: InstanceId,
    pub owner: super::ContentOwner,
    pub workspace: WorkspaceId,
    pub document: Option<DocumentId>,
    pub viewport: ViewportId,
    pub native_host: Option<HostId>,
    pub outer: LogicalRect,
    pub usable_body: LogicalRect,
    pub flow: Flow,
    pub size_class: SizeClass,
    pub focused: bool,
    pub direction: Direction,
    pub motion: Motion,
}
impl PanelContext {
    pub fn validate(&self) -> PanelResult<()> {
        valid_id(&self.instance.0)?;
        self.owner.validate()?;
        LocalScope { workspace: self.workspace.clone(), document: self.document.clone(), viewport: self.viewport.clone() }.validate()?;
        self.outer.validate()?;
        self.usable_body.validate()?;
        if !self.outer.contains_rect(self.usable_body) {
            return Err(error(Rule::Bounds, "presentation body outside host"));
        }
        if let Some(host) = &self.native_host {
            valid_id(&host.0)?;
            if self.viewport.0 != host.0 {
                return Err(error(Rule::Placement, "native presentation has wrong local viewport"));
            }
        }
        if let super::ContentOwner::Document(document) = &self.owner
            && self.document.as_ref() != Some(document)
        {
            return Err(error(Rule::Placement, "presentation document differs from content owner"));
        }
        if let super::ContentOwner::Workspace(workspace) = &self.owner
            && self.workspace != *workspace
        {
            return Err(error(Rule::Placement, "presentation workspace differs from content owner"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Reduced,
    Animated,
}
impl Motion {
    /// Reparent/focus must not wait for animation completion, including at zero duration.
    pub fn duration_ms(self, requested: u16) -> u16 {
        if self == Self::Reduced { 0 } else { requested.min(500) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocalScope {
    pub workspace: WorkspaceId,
    pub document: Option<DocumentId>,
    pub viewport: ViewportId,
}
impl LocalScope {
    pub fn validate(&self) -> PanelResult<()> {
        valid_id(&self.workspace.0)?;
        valid_id(&self.viewport.0)?;
        if let Some(id) = &self.document {
            valid_id(&id.0)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum LocalRequest {
    KeyboardOwner { instance: InstanceId, control: ControlId },
    ReleaseKeyboard { instance: InstanceId },
    Focus { instance: InstanceId, control: ControlId },
    FileDrop { instance: InstanceId, name: String, bytes: Vec<u8> },
    PointerDrop { instance: InstanceId, at: Point, payload: String },
    Overlay { instance: InstanceId, region: InputRegion },
    ClearOverlays { instance: InstanceId },
    Clipboard { instance: InstanceId, text: String },
    Pick { instance: InstanceId, at: Point },
}
impl LocalRequest {
    pub fn instance(&self) -> &InstanceId {
        match self {
            Self::KeyboardOwner { instance, .. }
            | Self::ReleaseKeyboard { instance }
            | Self::Focus { instance, .. }
            | Self::FileDrop { instance, .. }
            | Self::PointerDrop { instance, .. }
            | Self::Overlay { instance, .. }
            | Self::ClearOverlays { instance }
            | Self::Clipboard { instance, .. }
            | Self::Pick { instance, .. } => instance,
        }
    }
    pub fn validate(&self) -> PanelResult<()> {
        valid_id(&self.instance().0)?;
        match self {
            Self::KeyboardOwner { control, .. } | Self::Focus { control, .. } => valid_id(&control.0)?,
            Self::FileDrop { name, bytes, .. } => {
                if name.is_empty() || name.len() > 256 || bytes.len() > MAX_ENVELOPE_BYTES {
                    return Err(error(Rule::CollectionLimit, "file drop bound"));
                }
            }
            Self::PointerDrop { at, payload, .. } => {
                at.validate()?;
                if payload.len() > 4096 {
                    return Err(error(Rule::CollectionLimit, "pointer drop payload bound"));
                }
            }
            Self::Overlay { region, .. } => {
                valid_id(&region.control.0)?;
                region.bounds.validate()?;
                if region.accessible_name.is_empty() || region.accessible_name.len() > 256 {
                    return Err(error(Rule::Bounds, "overlay semantic name bound"));
                }
            }
            Self::Clipboard { text, .. } if text.len() > MAX_ENVELOPE_BYTES => return Err(error(Rule::CollectionLimit, "clipboard bound")),
            Self::Pick { at, .. } => at.validate()?,
            _ => {}
        }
        Ok(())
    }
}

/// Services must reject cross-document/viewport delivery. A native redock drops
/// host-local focus/overlays but retains the content controller and its owner.
pub trait PanelLocalServices {
    fn request(&mut self, scope: &LocalScope, request: LocalRequest) -> PanelResult<()>;
    fn release(&mut self, scope: &LocalScope, instance: &InstanceId);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeAction {
    pub id: ControlId,
    pub label: String,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelPresentation {
    /// Already localised by content; fixtures use specimen text only.
    pub title: String,
    pub icon: String,
    pub status: Option<String>,
    pub attention: bool,
    pub actions: Vec<RuntimeAction>,
}
impl PanelPresentation {
    pub fn validate(&self) -> PanelResult<()> {
        if self.title.is_empty() || self.title.len() > 256 || self.status.as_ref().is_some_and(|s| s.len() > 1024) || self.actions.len() > MAX_TABS {
            return Err(error(Rule::CollectionLimit, "presentation bound"));
        }
        valid_id(&self.icon)?;
        let mut ids = std::collections::BTreeSet::new();
        for action in &self.actions {
            valid_id(&action.id.0)?;
            if !ids.insert(&action.id) {
                return Err(error(Rule::DuplicateId, "duplicate content action"));
            }
            if action.label.is_empty()
                || action.label.len() > 256
                || action.disabled_reason.as_ref().is_some_and(|r| r.is_empty() || r.len() > 256)
                || !action.enabled && action.disabled_reason.is_none()
            {
                return Err(error(Rule::Bounds, "content action label/reason bound"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentSlot {
    Header,
    Body,
    Footer,
}

/// Shared host adapter seam. Geometry is resolved by the host; content chooses
/// its own scrolling within slots. The egui adapter below owns one kit Container.
pub trait PanelHost {
    fn mount(
        &mut self,
        geometry: &ResolvedPanel,
        presentation: &PanelPresentation,
        context: &PanelContext,
        content: &mut dyn PanelController,
        services: &mut ContentServices<'_>,
    ) -> PanelResult<()>;
}

/// Two-phase host effects. `prepare` is side-effect-free and fallible; `commit`
/// cannot fail. Native adapters must stage resources before acknowledging prepare
/// and retain/restorable resources until the layout transaction commits.
pub trait NativeHostAdapter {
    fn capabilities(&self) -> HostCapabilities;
    fn prepare(&self, effects: &[HostEffect]) -> PanelResult<()>;
    fn commit(&mut self, effects: &[HostEffect]);
}

/// Existing ui_kit is the outer boundary. This adapter paints into host-provided
/// space; it does not invent windows, docking gestures or a second camera path.
pub struct EguiPanelHost<'a> {
    pub ui: &'a mut egui::Ui,
}

impl PanelHost for EguiPanelHost<'_> {
    fn mount(
        &mut self,
        geometry: &ResolvedPanel,
        presentation: &PanelPresentation,
        context: &PanelContext,
        content: &mut dyn PanelController,
        services: &mut ContentServices<'_>,
    ) -> PanelResult<()> {
        geometry.validate()?;
        presentation.validate()?;
        context.validate()?;
        if geometry.instance != context.instance || geometry.outer != context.outer || geometry.body != context.usable_body {
            return Err(error(Rule::Placement, "host geometry differs from presentation context"));
        }
        // This reference adapter draws/interacts with one full rectangular card.
        // Rich sparse/translucent regions belong to the surface host adapter.
        if geometry.painted.len() != 1
            || geometry.painted.first() != Some(&geometry.outer)
            || geometry.input.len() != 1
            || !geometry.input.first().is_some_and(|region| region.role == InputRole::Surface && region.bounds == geometry.outer)
        {
            return Err(error(Rule::Capability, "reference egui host requires one full painted/input surface"));
        }
        let rect = |r: LogicalRect| egui::Rect::from_min_size(egui::pos2(r.origin.x, r.origin.y), egui::vec2(r.size.width, r.size.height));
        let outer = rect(geometry.outer);
        let body = rect(geometry.body);
        let parent_clip = self.ui.clip_rect().intersect(outer);
        let container = crate::ui_kit::Container::new(self.ui.ctx(), crate::ui_kit::Surface::Panel);
        let frame = container.frame(self.ui.ctx(), crate::theme::Tokens::get().dialog_body, crate::frame::Side::Left).inner_margin(0);
        let mut failure = None;
        self.ui.scope_builder(
            egui::UiBuilder::new().id_salt(("panel_runtime", &geometry.instance.0, &context.owner, &context.viewport.0)).max_rect(outer),
            |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(parent_clip));
                container.show_frame(ui, frame, outer, |ui| {
                    // Slots have explicit bounds. No mandatory ScrollArea around content.
                    let header = egui::Rect::from_min_max(outer.min, egui::pos2(outer.right(), body.top()));
                    let footer = egui::Rect::from_min_max(egui::pos2(outer.left(), body.bottom()), outer.max);
                    for (slot, bounds) in [(ContentSlot::Header, header), (ContentSlot::Body, body), (ContentSlot::Footer, footer)] {
                        if failure.is_some() {
                            break;
                        }
                        ui.scope_builder(egui::UiBuilder::new().id_salt(slot as u8).max_rect(bounds), |ui| {
                            ui.set_clip_rect(ui.clip_rect().intersect(bounds));
                            if slot == ContentSlot::Header {
                                // Dynamic title/status have semantic metadata; content owns
                                // additional controls and returns requests through services.
                                let tokens = crate::theme::Tokens::get();
                                let ink = if presentation.attention { tokens.form_primary } else { tokens.form_ink };
                                let mut response = ui.label(egui::RichText::new(&presentation.title).color(ink));
                                if let Some(status) = &presentation.status {
                                    response = response.backing_on_hover_text(status);
                                }
                                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &presentation.title));
                            }
                            if slot == ContentSlot::Footer && !presentation.actions.is_empty() {
                                egui::ScrollArea::horizontal().id_salt("panel_actions").show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        for action in &presentation.actions {
                                            let response = ui
                                                .push_id(&action.id.0, |ui| {
                                                    ui.add_enabled_ui(action.enabled, |ui| {
                                                        crate::ui_kit::button(ui, action.label.as_str(), crate::ui_kit::Button::Small)
                                                    })
                                                    .inner
                                                })
                                                .inner;
                                            let clicked = response.clicked();
                                            if let Some(reason) = &action.disabled_reason {
                                                response.backing_on_disabled_hover_text(reason);
                                            }
                                            if clicked
                                                && failure.is_none()
                                                && let Err(reason) = content.action(&context.owner, &action.id, services)
                                            {
                                                failure = Some(reason);
                                            }
                                        }
                                    });
                                });
                            }
                            if let Err(reason) = content.render(slot, ui, context, services) {
                                failure = Some(reason);
                            }
                        });
                    }
                    ui.set_min_size(outer.size());
                });
            },
        );
        if let Some(reason) = failure {
            return Err(reason);
        }
        Ok(())
    }
}

use super::{ContentServices, PanelController};
